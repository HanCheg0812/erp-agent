use anyhow::{Context, Result, bail, ensure};
use calamine::{DataType, Reader as _, Xlsx};
use quick_xml::{Reader, events::Event};
use std::{
    collections::HashMap,
    io::{Cursor, Read},
    time::Duration,
};
use tempfile::tempdir;
use tokio::{process::Command, time::timeout};
use zip::ZipArchive;

const MAX_ARCHIVE_BYTES: u64 = 32 * 1024 * 1024;
const TURN_TEXT_CHARS: usize = 20_000;
const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub struct ExtractedImage {
    pub label: String,
    pub mime_type: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Default)]
pub struct ExtractedDocument {
    pub text: String,
    pub images: Vec<ExtractedImage>,
    pub complete: bool,
    pub warnings: Vec<String>,
    pub next_offset: usize,
    pub next_page: Option<usize>,
    pub next_image_offset: Option<usize>,
    start_offset: usize,
    seen_chars: usize,
    limit_chars: usize,
}

fn append(doc: &mut ExtractedDocument, text: &str) {
    for ch in text.chars() {
        if doc.seen_chars >= doc.start_offset {
            if doc.next_offset - doc.start_offset < doc.limit_chars {
                doc.text.push(ch);
                doc.next_offset += 1;
            } else {
                doc.complete = false;
            }
        }
        doc.seen_chars += 1;
    }
}

fn segment(offset: usize, limit: usize) -> Result<ExtractedDocument> {
    ensure!(
        offset <= 50_000_000 && (1..=TURN_TEXT_CHARS).contains(&limit),
        "Invalid read range"
    );
    Ok(ExtractedDocument {
        complete: true,
        start_offset: offset,
        next_offset: offset,
        limit_chars: limit,
        ..Default::default()
    })
}

fn finish(doc: ExtractedDocument) -> Result<ExtractedDocument> {
    ensure!(
        doc.start_offset <= doc.seen_chars,
        "Read offset exceeds document length"
    );
    Ok(doc)
}

pub fn validate_office(kind: &str, bytes: &[u8]) -> Result<()> {
    ensure!(bytes.starts_with(b"PK\x03\x04"), "Invalid Office archive");
    let mut archive = ZipArchive::new(Cursor::new(bytes)).context("Invalid Office archive")?;
    ensure!(archive.len() <= 1024, "Too many Office archive entries");
    let mut expanded = 0u64;
    let mut required = false;
    for index in 0..archive.len() {
        let entry = archive.by_index(index)?;
        expanded = expanded.saturating_add(entry.size());
        ensure!(
            expanded <= MAX_ARCHIVE_BYTES,
            "Office archive expands beyond limit"
        );
        if (kind == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            && entry.name() == "word/document.xml")
            || (kind == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
                && entry.name() == "xl/workbook.xml")
        {
            required = true;
        }
    }
    ensure!(required, "Office document type does not match its contents");
    Ok(())
}

fn xml_text(bytes: &quick_xml::events::BytesText<'_>) -> Result<String> {
    Ok(quick_xml::escape::unescape(bytes.as_ref())?.into_owned())
}

fn attribute(event: &quick_xml::events::BytesStart<'_>, name: &str) -> Option<String> {
    event
        .attributes()
        .flatten()
        .find_map(|attr| (attr.key.as_ref() == name).then(|| attr.value.as_ref().to_owned()))
}

fn read_zip_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    limit: u64,
) -> Result<Vec<u8>> {
    let mut file = archive.by_name(path)?;
    ensure!(file.size() <= limit, "Office component is too large");
    let mut bytes = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn image_mime(path: &str, bytes: &[u8]) -> Option<&'static str> {
    match path.rsplit('.').next()?.to_ascii_lowercase().as_str() {
        "png" if bytes.starts_with(b"\x89PNG\r\n\x1a\n") => Some("image/png"),
        "jpg" | "jpeg" if bytes.starts_with(b"\xff\xd8\xff") => Some("image/jpeg"),
        "webp" if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") => {
            Some("image/webp")
        }
        _ => None,
    }
}

fn table_block(rows: &[Vec<String>]) -> String {
    let width = rows.iter().map(Vec::len).max().unwrap_or(0).min(64);
    let mut result = String::new();
    for (index, row) in rows.iter().enumerate() {
        result.push_str(&format!("row {}: ", index + 1));
        for column in 0..width {
            if column > 0 {
                result.push_str(" | ");
            }
            result.push_str(&format!(
                "c{}={}",
                column + 1,
                row.get(column)
                    .map(String::as_str)
                    .unwrap_or("")
                    .replace(['\n', '\r'], " ")
            ));
        }
        result.push('\n');
    }
    result
}

pub fn extract_docx(bytes: &[u8], include_images: bool) -> Result<ExtractedDocument> {
    extract_docx_segment(bytes, include_images, 0, TURN_TEXT_CHARS)
}

pub fn extract_docx_segment(
    bytes: &[u8],
    include_images: bool,
    offset: usize,
    limit: usize,
) -> Result<ExtractedDocument> {
    extract_docx_segment_with_images(bytes, include_images, offset, limit, 0)
}

pub fn extract_docx_segment_with_images(
    bytes: &[u8],
    include_images: bool,
    offset: usize,
    limit: usize,
    image_offset: usize,
) -> Result<ExtractedDocument> {
    const KIND: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
    validate_office(KIND, bytes)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))?;
    let xml = read_zip_entry(&mut archive, "word/document.xml", 16 * 1024 * 1024)?;
    let mut relationships = HashMap::new();
    if let Ok(rels) = read_zip_entry(&mut archive, "word/_rels/document.xml.rels", 512 * 1024) {
        let mut reader = Reader::from_reader(rels.as_slice());
        loop {
            match reader.read_event()? {
                Event::Start(event) | Event::Empty(event)
                    if event.name().as_ref().ends_with("Relationship") =>
                {
                    if let (Some(id), Some(target)) =
                        (attribute(&event, "Id"), attribute(&event, "Target"))
                    {
                        if !target.contains("..") && target.starts_with("media/") {
                            relationships.insert(id, format!("word/{target}"));
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
    }
    let mut doc = segment(offset, limit)?;
    let mut reader = Reader::from_reader(xml.as_slice());
    let mut paragraph = String::new();
    let mut cell = String::new();
    let mut row = Vec::new();
    let mut rows = Vec::new();
    let mut image_ids = Vec::new();
    let mut in_text = false;
    let mut in_table = false;
    let mut in_cell = false;
    let mut paragraph_number = 0;
    let mut table_number = 0;
    let mut heading = 0;
    loop {
        match reader.read_event()? {
            Event::Start(event) | Event::Empty(event) => match event.name().as_ref() {
                "w:p" => {
                    paragraph.clear();
                    heading = 0;
                }
                "w:pStyle" => {
                    if let Some(style) = attribute(&event, "w:val") {
                        heading = style
                            .strip_prefix("Heading")
                            .and_then(|n| n.parse::<usize>().ok())
                            .unwrap_or(0)
                            .min(6);
                    }
                }
                "w:t" => in_text = true,
                "w:tab" => paragraph.push('\t'),
                "w:br" => paragraph.push('\n'),
                "w:tbl" => {
                    in_table = true;
                    rows.clear();
                }
                "w:tr" => row.clear(),
                "w:tc" => {
                    in_cell = true;
                    cell.clear();
                }
                "a:blip" => {
                    if let Some(id) = attribute(&event, "r:embed") {
                        image_ids.push(id);
                        paragraph.push_str(&format!(" [image {}] ", image_ids.len()));
                    }
                }
                _ => {}
            },
            Event::Text(text) if in_text => paragraph.push_str(&xml_text(&text)?),
            Event::GeneralRef(reference) if in_text => {
                paragraph.push_str(&quick_xml::escape::unescape(&format!(
                    "&{};",
                    reference.as_ref()
                ))?);
            }
            Event::End(event) => match event.name().as_ref() {
                "w:t" => in_text = false,
                "w:p" => {
                    paragraph_number += 1;
                    if in_cell {
                        if !cell.is_empty() {
                            cell.push(' ');
                        }
                        cell.push_str(paragraph.trim());
                    } else if !paragraph.trim().is_empty() {
                        let title = if heading > 0 {
                            "#".repeat(heading)
                        } else {
                            String::new()
                        };
                        append(
                            &mut doc,
                            &format!(
                                "{title}[paragraph {paragraph_number}] {}\n",
                                paragraph.trim()
                            ),
                        );
                    }
                }
                "w:tc" => {
                    row.push(std::mem::take(&mut cell));
                    in_cell = false;
                }
                "w:tr" => rows.push(std::mem::take(&mut row)),
                "w:tbl" => {
                    table_number += 1;
                    append(
                        &mut doc,
                        &format!("[table {table_number}]\n{}", table_block(&rows)),
                    );
                    in_table = false;
                }
                _ => {}
            },
            Event::Eof => break,
            _ => {}
        }
    }
    ensure!(
        image_offset <= image_ids.len(),
        "Image offset exceeds document images"
    );
    if !include_images && !image_ids.is_empty() {
        doc.complete = false;
        doc.warnings
            .push("This model did not inspect the document images.".into());
    }
    let mut image_bytes = 0usize;
    if include_images {
        for (index, id) in image_ids.iter().enumerate().skip(image_offset) {
            if doc.images.len() == 4 {
                doc.complete = false;
                doc.next_image_offset = Some(index);
                doc.warnings
                    .push(format!("Continue document images at image_offset {index}."));
                break;
            }
            let Some(path) = relationships.get(id) else {
                doc.complete = false;
                doc.warnings
                    .push(format!("Image {} has no readable relationship.", index + 1));
                continue;
            };
            let raw = read_zip_entry(&mut archive, path, MAX_IMAGE_BYTES as u64)?;
            let Some(mime_type) = image_mime(path, &raw) else {
                doc.complete = false;
                doc.warnings
                    .push(format!("Image {} uses an unsupported format.", index + 1));
                continue;
            };
            if image_bytes + raw.len() > MAX_IMAGE_BYTES {
                doc.complete = false;
                doc.next_image_offset = Some(index);
                doc.warnings.push(format!(
                    "Continue document images at image_offset {index}; image budget reached."
                ));
                break;
            }
            image_bytes += raw.len();
            doc.images.push(ExtractedImage {
                label: format!("image {}", index + 1),
                mime_type: mime_type.into(),
                bytes: raw,
            });
        }
    }
    if in_table {
        bail!("Unclosed Word table");
    }
    finish(doc)
}

fn column_label(mut index: usize) -> String {
    let mut label = String::new();
    loop {
        label.insert(0, (b'A' + (index % 26) as u8) as char);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    label
}

pub fn extract_xlsx(bytes: &[u8]) -> Result<ExtractedDocument> {
    extract_xlsx_segment(bytes, 0, TURN_TEXT_CHARS)
}

pub fn extract_xlsx_segment(
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Result<ExtractedDocument> {
    const KIND: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";
    validate_office(KIND, bytes)?;
    let mut workbook = Xlsx::new(Cursor::new(bytes))?;
    let names = workbook.sheet_names().to_vec();
    let mut doc = segment(offset, limit)?;
    for name in names {
        let range = workbook.worksheet_range(&name)?;
        let formulas = workbook.worksheet_formula(&name)?;
        append(&mut doc, &format!("[sheet {name}]\n"));
        let (start_row, start_col) = range.start().unwrap_or((0, 0));
        for (r, row) in range.rows().enumerate() {
            let mut cells = Vec::new();
            for (c, value) in row.iter().enumerate() {
                let actual_row = start_row as usize + r;
                let actual_col = start_col as usize + c;
                let formula = formulas
                    .get_value((actual_row as u32, actual_col as u32))
                    .filter(|text| !text.is_empty());
                if value.is_empty() && formula.is_none() {
                    continue;
                }
                let cell = format!("{}{}", column_label(actual_col), actual_row + 1);
                let mut rendered = value.to_string().replace(['\n', '\r'], " ");
                if let Some(formula) = formula {
                    rendered.push_str(&format!(" [formula: {formula}]"));
                }
                cells.push(format!("{cell}={rendered}"));
            }
            if !cells.is_empty() {
                append(
                    &mut doc,
                    &format!(
                        "row {} | {}\n",
                        start_row as usize + r + 1,
                        cells.join(" | ")
                    ),
                );
            }
        }
    }
    finish(doc)
}

pub fn extract_text(kind: &str, bytes: &[u8]) -> Result<ExtractedDocument> {
    extract_text_segment(kind, bytes, 0, TURN_TEXT_CHARS)
}

pub fn extract_text_segment(
    kind: &str,
    bytes: &[u8],
    offset: usize,
    limit: usize,
) -> Result<ExtractedDocument> {
    ensure!(
        kind == "text/plain" || kind == "text/markdown",
        "Unsupported text type"
    );
    let text = std::str::from_utf8(bytes)?.trim_start_matches('\u{feff}');
    let mut doc = segment(offset, limit)?;
    append(&mut doc, text);
    if !doc.complete {
        doc.warnings
            .push("Text was truncated for this turn.".into());
    }
    finish(doc)
}

async fn checked_command(command: &mut Command) -> Result<()> {
    let output = timeout(Duration::from_secs(20), command.kill_on_drop(true).output())
        .await
        .context("Document conversion timed out")??;
    ensure!(
        output.status.success(),
        "Document conversion failed: {}",
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(300)
            .collect::<String>()
    );
    Ok(())
}

pub async fn extract_pdf(bytes: &[u8], include_images: bool) -> Result<ExtractedDocument> {
    extract_pdf_pages(bytes, include_images, 1, 4, 0).await
}

pub async fn extract_pdf_pages(
    bytes: &[u8],
    include_images: bool,
    first_page: usize,
    count: usize,
    offset: usize,
) -> Result<ExtractedDocument> {
    ensure!(
        first_page > 0 && count > 0 && count <= 4,
        "Invalid PDF page range"
    );
    ensure!(bytes.starts_with(b"%PDF-"), "Invalid PDF");
    let dir = tempdir()?;
    let source = dir.path().join("source.pdf");
    let text_file = dir.path().join("text.txt");
    tokio::fs::write(&source, bytes).await?;
    let info = timeout(
        Duration::from_secs(10),
        Command::new("pdfinfo")
            .arg(&source)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .context("PDF inspection timed out")??;
    ensure!(info.status.success(), "PDF inspection failed");
    let pages: usize = String::from_utf8_lossy(&info.stdout)
        .lines()
        .find_map(|line| {
            line.strip_prefix("Pages:")
                .and_then(|value| value.trim().parse().ok())
        })
        .context("PDF page count missing")?;
    ensure!(pages > 0 && pages <= 500, "PDF page limit exceeded");
    ensure!(first_page <= pages, "PDF page is out of range");
    let last_page = (first_page + count - 1).min(pages);
    checked_command(
        Command::new("pdftotext")
            .args([
                "-f",
                &first_page.to_string(),
                "-l",
                &last_page.to_string(),
                "-layout",
                "-enc",
                "UTF-8",
            ])
            .arg(&source)
            .arg(&text_file),
    )
    .await?;
    ensure!(
        tokio::fs::metadata(&text_file).await?.len() <= 2 * 1024 * 1024,
        "PDF text exceeds limit"
    );
    let extracted = tokio::fs::read_to_string(&text_file).await?;
    let mut doc = segment(offset, TURN_TEXT_CHARS)?;
    doc.complete = last_page == pages;
    let page_text: Vec<_> = extracted
        .split('\u{000c}')
        .take(last_page - first_page + 1)
        .collect();
    for (index, text) in page_text.iter().enumerate() {
        if !text.trim().is_empty() {
            append(
                &mut doc,
                &format!("[PDF page {}]\n{}\n", first_page + index, text.trim()),
            );
        }
    }
    if include_images && offset == 0 {
        let mut total = 0usize;
        for page in first_page..=last_page {
            let prefix = dir.path().join(format!("page-{page}"));
            checked_command(
                Command::new("pdftoppm")
                    .args([
                        "-f",
                        &page.to_string(),
                        "-l",
                        &page.to_string(),
                        "-scale-to",
                        "1400",
                        "-png",
                        "-singlefile",
                    ])
                    .arg(&source)
                    .arg(&prefix),
            )
            .await?;
            let image = tokio::fs::read(prefix.with_extension("png")).await?;
            if total + image.len() > MAX_IMAGE_BYTES {
                doc.complete = false;
                doc.warnings.push(format!("PDF page {page} image was omitted by the image budget; retry that page with page_count 1."));
                break;
            }
            total += image.len();
            doc.images.push(ExtractedImage {
                label: format!("PDF page {page}"),
                mime_type: "image/png".into(),
                bytes: image,
            });
        }
    }
    if doc.seen_chars > doc.next_offset {
        doc.next_page = Some(first_page);
        doc.warnings.push(format!(
            "Continue PDF text at page {first_page}, offset {}.",
            doc.next_offset
        ));
    } else if last_page < pages {
        doc.next_page = Some(last_page + 1);
        doc.next_offset = 0;
        doc.warnings.push(format!("Pages {first_page}-{last_page} of {pages} were read; continue at page {} with offset 0.", last_page + 1));
    }
    if page_text.iter().all(|text| text.trim().is_empty()) && doc.images.is_empty() && offset == 0 {
        doc.complete = false;
        doc.warnings
            .push("No PDF text was extracted and no page images were sent to this model.".into());
    } else if page_text.iter().any(|text| text.trim().is_empty()) {
        if !include_images {
            doc.complete = false;
        }
        doc.warnings.push(
            "Some pages have no extractable text; verify them visually before using their facts."
                .into(),
        );
    }
    finish(doc)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn archive(files: &[(&str, &str)]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, content) in files {
            writer
                .start_file(*name, SimpleFileOptions::default())
                .unwrap();
            writer.write_all(content.as_bytes()).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn two_page_pdf() -> Vec<u8> {
        let first = "BT /F1 18 Tf 72 700 Td (First page) Tj ET";
        let second = "BT /F1 18 Tf 72 700 Td (Second page) Tj ET";
        let objects = [
            "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
            "<< /Type /Pages /Kids [3 0 R 5 0 R] /Count 2 >>".to_string(),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 4 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{first}\nendstream", first.len()),
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 7 0 R >> >> /Contents 6 0 R >>".to_string(),
            format!("<< /Length {} >>\nstream\n{second}\nendstream", second.len()),
            "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".to_string(),
        ];
        let mut pdf = b"%PDF-1.4\n".to_vec();
        let mut offsets = vec![0];
        for (index, object) in objects.iter().enumerate() {
            offsets.push(pdf.len());
            pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", index + 1).as_bytes());
        }
        let xref = pdf.len();
        pdf.extend_from_slice(
            format!("xref\n0 {}\n0000000000 65535 f \n", offsets.len()).as_bytes(),
        );
        for offset in offsets.iter().skip(1) {
            pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
        }
        pdf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                offsets.len()
            )
            .as_bytes(),
        );
        pdf
    }

    #[test]
    fn text_cursor_reads_all_content_without_repeating_or_losing_unicode() {
        let input = "汉字A".repeat(12_000);
        let first = extract_text_segment("text/markdown", input.as_bytes(), 0, 20_000).unwrap();
        assert!(!first.complete);
        assert_eq!(first.next_offset, 20_000);
        let second =
            extract_text_segment("text/markdown", input.as_bytes(), first.next_offset, 20_000)
                .unwrap();
        assert!(second.complete);
        assert_eq!(format!("{}{}", first.text, second.text), input);
        assert!(extract_text_segment("text/markdown", input.as_bytes(), 40_000, 20_000).is_err());
    }

    #[test]
    fn word_paragraphs_and_tables_keep_source_positions() {
        let file = archive(&[(
            "word/document.xml",
            r#"<w:document xmlns:w="w"><w:body><w:p><w:r><w:t>Alpha &amp; Beta</w:t></w:r></w:p><w:tbl><w:tr><w:tc><w:p><w:r><w:t>Item</w:t></w:r></w:p></w:tc><w:tc><w:p><w:r><w:t>1280</w:t></w:r></w:p></w:tc></w:tr></w:tbl></w:body></w:document>"#,
        )]);
        let first = extract_docx_segment(&file, false, 0, 20).unwrap();
        let second = extract_docx_segment(&file, false, first.next_offset, 20_000).unwrap();
        let combined = format!("{}{}", first.text, second.text);
        assert!(
            combined.contains("[paragraph 1] Alpha & Beta"),
            "{combined:?}"
        );
        assert!(combined.contains("[table 1]"));
        assert!(combined.contains("c1=Item | c2=1280"));
        assert!(second.complete);
    }

    #[test]
    fn word_images_continue_after_the_first_four() {
        let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        let blips = (1..=5)
            .map(|n| format!("<a:blip r:embed=\"rId{n}\"/>"))
            .collect::<String>();
        writer.start_file("word/document.xml", options).unwrap();
        writer.write_all(format!("<w:document xmlns:w=\"w\" xmlns:a=\"a\" xmlns:r=\"r\"><w:body><w:p>{blips}</w:p></w:body></w:document>").as_bytes()).unwrap();
        let rels = (1..=5)
            .map(|n| format!("<Relationship Id=\"rId{n}\" Target=\"media/{n}.png\"/>"))
            .collect::<String>();
        writer
            .start_file("word/_rels/document.xml.rels", options)
            .unwrap();
        writer
            .write_all(format!("<Relationships>{rels}</Relationships>").as_bytes())
            .unwrap();
        for n in 1..=5 {
            writer
                .start_file(format!("word/media/{n}.png"), options)
                .unwrap();
            writer.write_all(b"\x89PNG\r\n\x1a\n").unwrap();
        }
        let file = writer.finish().unwrap().into_inner();
        let first = extract_docx_segment_with_images(&file, true, 0, 20_000, 0).unwrap();
        assert_eq!(first.images.len(), 4);
        assert_eq!(first.next_image_offset, Some(4));
        let second =
            extract_docx_segment_with_images(&file, true, first.next_offset, 20_000, 4).unwrap();
        assert_eq!(second.images.len(), 1);
        assert_eq!(second.images[0].label, "image 5");
        assert!(second.next_image_offset.is_none());
    }

    #[tokio::test]
    async fn pdf_page_cursor_advances_without_repeating_text() {
        let file = two_page_pdf();
        let first = extract_pdf_pages(&file, false, 1, 1, 0).await.unwrap();
        assert!(first.text.contains("First page"));
        assert!(!first.text.contains("Second page"));
        assert_eq!(first.next_page, Some(2));
        assert_eq!(first.next_offset, 0);
        let second =
            extract_pdf_pages(&file, false, first.next_page.unwrap(), 1, first.next_offset)
                .await
                .unwrap();
        assert!(second.text.contains("Second page"));
        assert!(!second.text.contains("First page"));
        assert!(second.complete);
    }

    #[test]
    fn spreadsheet_keeps_sheet_and_cell_coordinates() {
        let file = archive(&[
            (
                "[Content_Types].xml",
                r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#,
            ),
            (
                "_rels/.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#,
            ),
            (
                "xl/workbook.xml",
                r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Orders" sheetId="1" r:id="rId1"/></sheets></workbook>"#,
            ),
            (
                "xl/_rels/workbook.xml.rels",
                r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#,
            ),
            (
                "xl/worksheets/sheet1.xml",
                r#"<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData><row r="3"><c r="B3" t="inlineStr"><is><t>Item</t></is></c><c r="C3"><v>1280</v></c></row></sheetData></worksheet>"#,
            ),
        ]);
        let extracted = extract_xlsx(&file).unwrap();
        assert!(extracted.text.contains("[sheet Orders]"));
        assert!(extracted.text.contains("B3=Item"));
        assert!(extracted.text.contains("C3=1280"));
    }
}
