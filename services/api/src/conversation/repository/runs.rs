use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn active_run<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<bool> {
    Ok(db.query_opt("SELECT 1 FROM agent_runs WHERE tenant_id=$1 AND case_id=$2 AND state IN ('queued','running','waiting')", &[&actor.tenant_id, &id]).await?.is_some())
}

pub(crate) async fn enqueue_run(
    tx: &Transaction<'_>,
    message_id: i64,
    actor: &Principal,
    id: Uuid,
    operation: &str,
    settings: &Value,
) -> Result<()> {
    tx.execute("INSERT INTO agent_runs(message_id,tenant_id,case_id,operation,settings) VALUES($1,$2,$3,$4,$5)", &[&message_id, &actor.tenant_id, &id, &operation, settings]).await?;
    Ok(())
}

pub(crate) async fn run_for_message<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    message: i64,
) -> Result<(String, bool)> {
    db.query_opt("SELECT state,cancel_requested FROM agent_runs WHERE tenant_id=$1 AND case_id=$2 AND message_id=$3 FOR UPDATE", &[&actor.tenant_id, &id, &message])
        .await?
        .map(|row| (row.get(0), row.get(1)))
        .ok_or_else(Error::missing)
}

pub(crate) async fn cancel_run(tx: &Transaction<'_>, message: i64, state: &str) -> Result<()> {
    tx.execute(
        "UPDATE agent_runs SET cancel_requested=true,state=$2,updated_at=now() WHERE message_id=$1",
        &[&message, &state],
    )
    .await?;
    tx.execute("UPDATE agent_tool_permissions SET state='cancelled',decided_by='cancel',reason='用户停止了本次回复',decided_at=now() WHERE message_id=$1 AND state IN ('pending','allowed')", &[&message]).await?;
    Ok(())
}
