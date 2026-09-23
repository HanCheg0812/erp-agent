use super::super::repository::memory as repository;
use crate::{auth::Principal, platform::*};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
pub async fn memory_list(app: App, actor: Principal) -> Result<Value> {
    let db = app.db.get().await?;
    Ok(json!({"items":memories(&**db,&actor).await?}))
}
pub async fn memories<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
) -> Result<Vec<Value>> {
    repository::list(db, actor).await
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Memory {
    request_id: Uuid,
    text: String,
}
pub async fn memory_save(app: App, actor: Principal, value: Value) -> Result<(bool, Value)> {
    let input: Memory = parse(value)?;
    let text_value = input.text.trim();
    text(text_value, 2000)?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    repository::lock_owner(&tx, &actor).await?;
    if let Some((tenant, owner, prior_text)) = repository::by_request(&tx, input.request_id).await?
    {
        if tenant != actor.tenant_id || owner != actor.user_id || prior_text != text_value {
            return Err(Error::conflict("idempotency_conflict"));
        }
        return Ok((false, json!({"id":input.request_id,"text":text_value})));
    }
    if repository::count(&tx, &actor).await? >= 50 {
        return Err(Error::bad("Memory limit reached; remove an entry first"));
    }
    let value = repository::insert(&tx, &actor, input.request_id, text_value).await?;
    tx.commit().await?;
    Ok((true, value))
}
pub async fn memory_delete(app: App, actor: Principal, id: Uuid) -> Result<()> {
    let db = app.db.get().await?;
    if !repository::delete(&**db, &actor, id).await? {
        return Err(Error::missing());
    }
    Ok(())
}

// Joins the permission execution transaction: no independent commit or replay.
pub(crate) async fn save_agent_memory_tx(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    value: &str,
) -> Result<Value> {
    text(value, 2000)?;
    repository::lock_owner(tx, actor).await?;
    if repository::count(tx, actor).await? >= 50 {
        return Err(Error::conflict("memory_limit"));
    }
    repository::insert(tx, actor, id, value.trim()).await?;
    Ok(json!({"status":"saved","memory_id":id,"text":value.trim()}))
}
