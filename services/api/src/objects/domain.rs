use crate::platform::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Upload {
    request_id: Uuid,
    filename: String,
    content_type: String,
    data_base64: String,
}
fn valid_content(kind: &str, filename: &str, data: &[u8]) -> bool {
    let extension = filename
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    match kind {
        "application/pdf" => extension == "pdf" && data.starts_with(b"%PDF-"),
        "image/png" => extension == "png" && data.starts_with(b"\x89PNG\r\n\x1a\n"),
        "image/jpeg" => {
            ["jpg", "jpeg"].contains(&extension.as_str()) && data.starts_with(b"\xff\xd8\xff")
        }
        "image/webp" => {
            extension == "webp" && data.starts_with(b"RIFF") && data.get(8..12) == Some(b"WEBP")
        }
        "text/plain" => extension == "txt" && std::str::from_utf8(data).is_ok(),
        "text/markdown" => extension == "md" && std::str::from_utf8(data).is_ok(),
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            if extension == "docx" =>
        {
            erp_ingestion::extract::validate_office(kind, data).is_ok()
        }
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            if extension == "xlsx" =>
        {
            erp_ingestion::extract::validate_office(kind, data).is_ok()
        }
        _ => false,
    }
}

pub(super) struct UploadDocument {
    pub(super) request_id: Uuid,
    pub(super) filename: String,
    pub(super) content_type: String,
    pub(super) data: Vec<u8>,
    pub(super) hash: String,
}
pub(super) fn upload_input(value: Value) -> Result<UploadDocument> {
    let input: Upload = parse(value)?;
    text(&input.filename, 180)?;
    if input
        .filename
        .chars()
        .any(|c| c.is_control() || c == '/' || c == '\\')
        || input.data_base64.len() > 7 * 1024 * 1024
    {
        return Err(Error::bad("Invalid document"));
    }
    let data = STANDARD
        .decode(&input.data_base64)
        .map_err(|_| Error::bad("Invalid file encoding"))?;
    if data.is_empty()
        || data.len() > 5 * 1024 * 1024
        || !valid_content(&input.content_type, &input.filename, &data)
    {
        return Err(Error::bad(
            "Use a PDF, DOCX, XLSX, MD, TXT, PNG, JPEG or WebP file up to 5 MB",
        ));
    }

    Ok(UploadDocument {
        request_id: input.request_id,
        filename: input.filename,
        content_type: input.content_type,
        hash: digest(&data),
        data,
    })
}
pub(super) fn check_upload_retry(prior: &Value, input: &UploadDocument, key: &str) -> Result<()> {
    if prior["object_key"] != key
        || prior["sha256"] != input.hash
        || prior["filename"] != input.filename
        || prior["content_type"] != input.content_type
    {
        return Err(Error::conflict("idempotency_conflict"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn upload() -> Value {
        json!({"request_id":Uuid::nil(),"filename":"note.txt","content_type":"text/plain","data_base64":STANDARD.encode("synthetic")})
    }
    #[test]
    fn upload_validation_checks_path_encoding_type_and_size() {
        assert!(upload_input(upload()).is_ok());
        for name in ["../note", "a/b", "a\\b", "a\nb"] {
            let mut v = upload();
            v["filename"] = json!(name);
            assert!(upload_input(v).is_err());
        }
        for (field, value) in [
            ("data_base64", json!("!!!")),
            ("data_base64", json!("")),
            ("content_type", json!("image/png")),
            ("content_type", json!("text/html")),
            ("extra", json!(true)),
        ] {
            let mut v = upload();
            v[field] = value;
            assert!(upload_input(v).is_err());
        }
        let mut v = upload();
        v["data_base64"] = json!(STANDARD.encode(vec![b'x'; 5 * 1024 * 1024 + 1]));
        assert!(upload_input(v).is_err());
    }
    #[test]
    fn upload_retry_requires_exact_scope_hash_name_and_type() {
        let input = upload_input(upload()).unwrap();
        let prior = json!({"object_key":"tenant/user/case/request/source","sha256":input.hash,"filename":input.filename,"content_type":input.content_type});
        assert!(check_upload_retry(&prior, &input, "tenant/user/case/request/source").is_ok());
        for field in ["object_key", "sha256", "filename", "content_type"] {
            let mut changed = prior.clone();
            changed[field] = json!("changed");
            assert!(
                check_upload_retry(&changed, &input, "tenant/user/case/request/source").is_err()
            );
        }
    }
}
