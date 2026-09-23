use crate::platform::*;
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;
pub(crate) async fn pending<C: GenericClient + Sync>(db: &C, mid: i64) -> Result<Vec<Value>> {
    array(db,"SELECT to_jsonb(p)||jsonb_build_object('user_task',m.text) FROM agent_tool_permissions p JOIN case_conversation_messages m ON m.id=p.message_id WHERE p.message_id=$1 AND p.state='pending' ORDER BY p.created_at", &[&mid]).await
}
pub(crate) async fn case<C: GenericClient + Sync>(
    db: &C,
    case: Uuid,
    tenant: Uuid,
    owner: Uuid,
) -> Result<Value> {
    object(
        db,
        "SELECT to_jsonb(c) FROM cases c WHERE id=$1 AND tenant_id=$2 AND owner_id=$3",
        &[&case, &tenant, &owner],
    )
    .await
}
pub(crate) async fn recent_messages<C: GenericClient + Sync>(
    db: &C,
    case: Uuid,
    mid: i64,
) -> Result<Vec<Value>> {
    array(db,"SELECT jsonb_build_object('text',text) FROM case_conversation_messages WHERE case_id=$1 AND id<=$2 ORDER BY id DESC LIMIT 10", &[&case,&mid]).await
}
pub(crate) async fn lock_waiting(tx: &Transaction<'_>, mid: i64) -> Result<bool> {
    Ok(tx.query_opt("SELECT 1 FROM agent_runs WHERE message_id=$1 AND state='waiting' AND NOT cancel_requested FOR UPDATE", &[&mid]).await?.is_some())
}
pub(crate) async fn record_review(
    tx: &Transaction<'_>,
    id: Uuid,
    decision: &str,
    source: &str,
    reason: &str,
) -> Result<u64> {
    Ok(tx.execute("UPDATE agent_tool_permissions SET state=$2,decided_by=$3,reason=$4,decided_at=now() WHERE id=$1 AND state='pending'", &[&id,&decision,&source,&reason]).await?)
}
pub(crate) async fn audit_review(tx: &Transaction<'_>, id: Uuid) -> Result<()> {
    tx.execute("INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) SELECT tenant_id,case_id,owner_id,'agent.permission_decided',jsonb_build_object('permission_id',id,'decision',state,'source',decided_by,'reason',reason) FROM agent_tool_permissions WHERE id=$1", &[&id]).await?;
    Ok(())
}
pub(crate) async fn resume_if_resolved<C: GenericClient + Sync>(db: &C, mid: i64) -> Result<()> {
    db.execute("UPDATE agent_runs SET state='queued',updated_at=now() WHERE message_id=$1 AND state='waiting' AND NOT cancel_requested AND NOT EXISTS(SELECT 1 FROM agent_tool_permissions WHERE message_id=$1 AND state='pending')", &[&mid]).await?;
    Ok(())
}
