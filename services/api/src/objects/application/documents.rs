use super::super::{
    domain,
    repository::documents as repository,
    storage::{self, unavailable},
};
use crate::{auth::Principal, cases, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) struct Download {
    pub(crate) content_type: String,
    pub(crate) bytes: Vec<u8>,
}
pub async fn list(app: App, actor: Principal, id: Uuid) -> Result<Value> {
    let db = app.db.get().await?;
    cases::case(&**db, &actor, id, false).await?;
    let items = repository::list(&**db, &actor, id).await?;
    Ok(json!({"items":items,"configured":app.bucket.is_some()}))
}
pub async fn upload(
    app: App,
    actor: Principal,
    case_id: Uuid,
    value: Value,
) -> Result<(bool, Value)> {
    actor.require("planner")?;
    let input = domain::upload_input(value)?;
    let bucket = app.bucket.as_ref().ok_or_else(unavailable)?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, &actor, case_id, true).await?;
    let key = format!(
        "{}/{}/{}/{}/source",
        actor.tenant_id, actor.user_id, case_id, input.request_id
    );
    if let Some(prior) = repository::by_request(&tx, input.request_id).await? {
        domain::check_upload_retry(&prior, &input, &key)?;
        if prior["state"] == "ready" {
            let mut view = prior;
            for key in ["object_key", "tenant_id", "owner_id"] {
                view.as_object_mut().unwrap().remove(key);
            }
            return Ok((false, view));
        }
    } else {
        repository::insert_upload(&tx, &actor, case_id, &key, &input).await?;
    }
    tx.commit().await?;
    if storage::put(bucket, &key, &input.data, &input.content_type)
        .await
        .is_err()
    {
        repository::mark_failed(&**db, input.request_id).await?;
        return Err(unavailable());
    }
    let saved = repository::mark_ready(&**db, input.request_id).await?;
    Ok((true, saved))
}
pub async fn download(app: App, actor: Principal, id: Uuid) -> Result<Download> {
    let db = app.db.get().await?;
    let doc = repository::ready(&**db, &actor, id).await?;
    let bucket = app.bucket.as_ref().ok_or_else(unavailable)?;
    let bytes = storage::get_verified(
        bucket,
        doc["object_key"].as_str().unwrap(),
        doc["sha256"].as_str().unwrap(),
    )
    .await?;
    Ok(Download {
        content_type: doc["content_type"].as_str().unwrap().to_owned(),
        bytes,
    })
}
