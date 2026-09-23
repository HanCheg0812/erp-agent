use super::super::{domain, repository};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn set_context(
    app: &App,
    actor: &Principal,
    id: Uuid,
    value: Value,
) -> Result<Value> {
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let record = set_context_tx(&tx, actor, id, value, false).await?;
    tx.commit().await?;
    Ok(record)
}

// Agent tool execution joins its permission transaction so both writes are atomic.
pub(crate) async fn set_context_tx(
    tx: &Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    value: Value,
    from_agent: bool,
) -> Result<Value> {
    actor.require("planner")?;
    let input = domain::context_input(value)?;
    let current = repository::find(tx, actor, id, true).await?;
    let fields = json!(input.context);
    let Some(version) = domain::context_transition(&current, &input)? else {
        return Ok(current);
    };
    if !from_agent && repository::has_active_run(tx, actor, id).await? {
        return Err(Error::conflict("agent_busy"));
    }
    let record = repository::update_context(tx, id, &input.context, version).await?;
    repository::audit(
        tx,
        actor,
        id,
        "case.context_added",
        json!({"version":version,"context":fields}),
    )
    .await?;
    Ok(record)
}
