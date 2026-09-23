use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;
pub(crate) async fn expired<C: GenericClient + Sync>(db: &C, id: Uuid) -> Result<bool> {
    Ok(db
        .query_one(
            "SELECT expires_at<=now() FROM agent_tool_permissions WHERE id=$1",
            &[&id],
        )
        .await?
        .get(0))
}
pub(crate) async fn message_id(
    tx: &Transaction<'_>,
    actor: &Principal,
    case: Uuid,
    id: Uuid,
) -> Result<i64> {
    Ok(tx.query_opt("SELECT message_id FROM agent_tool_permissions WHERE id=$1 AND tenant_id=$2 AND owner_id=$3 AND case_id=$4", &[&id,&actor.tenant_id,&actor.user_id,&case]).await?.ok_or_else(Error::missing)?.get(0))
}
pub(crate) async fn lock_run(tx: &Transaction<'_>, mid: i64) -> Result<(String, bool)> {
    let row = tx
        .query_one(
            "SELECT state,cancel_requested FROM agent_runs WHERE message_id=$1 FOR UPDATE",
            &[&mid],
        )
        .await?;
    Ok((row.get(0), row.get(1)))
}
pub(crate) async fn lock_permission(tx: &Transaction<'_>, id: Uuid) -> Result<Value> {
    object(
        tx,
        "SELECT to_jsonb(p) FROM agent_tool_permissions p WHERE id=$1 FOR UPDATE",
        &[&id],
    )
    .await
}
pub(crate) async fn record_decision(
    tx: &Transaction<'_>,
    id: Uuid,
    decision: &str,
    reason: &str,
) -> Result<()> {
    tx.execute("UPDATE agent_tool_permissions SET state=$2,decided_by='user',reason=$3,decided_at=now() WHERE id=$1", &[&id,&decision,&reason]).await?;
    Ok(())
}
pub(crate) async fn resume(tx: &Transaction<'_>, mid: i64) -> Result<()> {
    tx.execute("UPDATE agent_runs SET state='queued',updated_at=now() WHERE message_id=$1 AND state='waiting'", &[&mid]).await?;
    Ok(())
}
