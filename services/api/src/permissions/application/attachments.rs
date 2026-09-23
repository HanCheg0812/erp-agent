use super::super::repository;
use super::session;
use crate::{agent_sessions, objects, platform::*};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadAttachment {
    pub document_id: Uuid,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "first_page")]
    pub page: usize,
    #[serde(default = "four_pages")]
    pub page_count: usize,
    #[serde(default)]
    pub image_offset: usize,
}

fn first_page() -> usize {
    1
}
fn four_pages() -> usize {
    4
}

pub(crate) async fn read(
    app: App,
    session_id: String,
    token: String,
    input: ReadAttachment,
) -> Result<Value> {
    if input.offset > 50_000_000
        || input.page == 0
        || input.page > 500
        || input.page_count == 0
        || input.page_count > 4
        || input.image_offset > 1024
    {
        return Err(Error::bad("Invalid attachment range"));
    }
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &session_id, &token, false).await?;
    let (actor, case_id) = session::principal(&tx, &session_id).await?;
    let attachment = repository::attachments::get(&*tx, &actor, case_id, input.document_id).await?;
    let settings = repository::session::settings(&*tx, &actor, case_id)
        .await?
        .unwrap_or_else(|| json!({"model":"deepseek-flash"}));
    let images_allowed = settings["model"] == "deepseek-flash";
    let filename = attachment.filename;
    let kind = attachment.kind;
    let bucket = app
        .bucket
        .as_ref()
        .ok_or(Error::bad("Document storage unavailable"))?;
    let bytes = objects::get_verified(bucket, &attachment.key, &attachment.hash).await?;
    if bytes.len() != attachment.size as usize {
        return Err(Error::bad("Attachment size changed"));
    }
    let mut warnings = Vec::new();
    let (text, next_offset, next_page, next_image_offset, complete, images) = if kind
        .starts_with("image/")
    {
        if !images_allowed {
            return Err(Error::bad("Image input requires DeepSeek Flash"));
        }
        (
            String::new(),
            0,
            None,
            None,
            true,
            vec![json!({"label":filename,"mime_type":kind,"data":STANDARD.encode(bytes)})],
        )
    } else {
        if kind != "application/pdf" && input.page != 1 {
            return Err(Error::bad("Use page only for PDF"));
        }
        if kind != "application/pdf" && input.page_count != 4 {
            return Err(Error::bad("Use page_count only for PDF"));
        }
        if kind != "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            && input.image_offset != 0
        {
            return Err(Error::bad("Use image_offset only for Word"));
        }
        let extracted = objects::read_document(
            &kind,
            bytes,
            images_allowed,
            input.offset,
            input.page,
            input.page_count,
            input.image_offset,
        )
        .await?;
        warnings = extracted.warnings;
        let images = extracted.images.into_iter().map(|image| json!({"label":image.label,"mime_type":image.mime_type,"data":STANDARD.encode(image.bytes)})).collect();
        (
            extracted.text,
            extracted.next_offset,
            extracted.next_page,
            extracted.next_image_offset,
            extracted.complete,
            images,
        )
    };
    repository::attachments::audit(&tx, &actor, case_id, &json!({"document_id":input.document_id,"page":input.page,"page_count":input.page_count,"offset":input.offset,"image_offset":input.image_offset})).await?;
    tx.commit().await?;
    Ok(
        json!({"document_id":input.document_id,"filename":filename,"content_type":kind,"text":text,"next_offset":next_offset,"next_page":next_page,"next_image_offset":next_image_offset,"complete":complete,"warnings":warnings,"images":images}),
    )
}
