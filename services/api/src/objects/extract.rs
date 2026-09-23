use erp_ingestion::extract::{self, ExtractedDocument};

pub(crate) async fn read(
    kind: &str,
    bytes: Vec<u8>,
    images: bool,
    offset: usize,
    page: usize,
    page_count: usize,
    image_offset: usize,
) -> anyhow::Result<ExtractedDocument> {
    match kind {
        "application/pdf" => {
            extract::extract_pdf_pages(&bytes, images, page, page_count, offset).await
        }
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document" => {
            tokio::task::spawn_blocking(move || {
                extract::extract_docx_segment_with_images(
                    &bytes,
                    images,
                    offset,
                    20_000,
                    image_offset,
                )
            })
            .await?
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet" => {
            tokio::task::spawn_blocking(move || {
                extract::extract_xlsx_segment(&bytes, offset, 20_000)
            })
            .await?
        }
        "text/plain" | "text/markdown" => {
            extract::extract_text_segment(kind, &bytes, offset, 20_000)
        }
        _ => anyhow::bail!("Unsupported document type"),
    }
}
