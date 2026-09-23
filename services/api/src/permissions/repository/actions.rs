use super::super::types::{Action, ActiveRun, ProposalRequest};
use crate::platform::*;
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;
pub(crate) async fn active_run(tx: &Transaction<'_>, session: &str) -> Result<ActiveRun> {
    let row = tx.query_opt("SELECT s.tenant_id,s.owner_id,s.case_id,r.message_id,r.settings FROM agent_sessions s JOIN case_conversations c ON c.session_id=s.id JOIN agent_runs r ON r.case_id=c.case_id AND r.tenant_id=c.tenant_id WHERE s.id=$1 AND r.state='running' AND NOT r.cancel_requested FOR UPDATE OF r", &[&session]).await?.ok_or_else(Error::forbidden)?;
    Ok(ActiveRun {
        tenant: row.get(0),
        owner: row.get(1),
        case: row.get(2),
        message: row.get(3),
        settings: row.get(4),
    })
}
pub(crate) async fn exists(tx: &Transaction<'_>, session: &str, call: &str) -> Result<bool> {
    Ok(tx
        .query_opt(
            "SELECT id FROM agent_tool_permissions WHERE session_id=$1 AND tool_call_id=$2",
            &[&session, &call],
        )
        .await?
        .is_some())
}
pub(crate) async fn proposal_matches(
    tx: &Transaction<'_>,
    run: &ActiveRun,
    request: &ProposalRequest,
) -> Result<bool> {
    Ok(tx.query_opt("SELECT 1 FROM proposals WHERE id=$1 AND tenant_id=$2 AND case_id=$3 AND case_version=$4 AND digest=$5", &[&request.proposal_id,&run.tenant,&run.case,&request.expected_case_version,&request.proposal_digest]).await?.is_some())
}
pub(crate) async fn insert(
    tx: &Transaction<'_>,
    run: &ActiveRun,
    session: &str,
    input: &Action,
    fingerprint: &str,
    mode: &str,
    proposal: Option<Uuid>,
) -> Result<()> {
    tx.execute("INSERT INTO agent_tool_permissions(tenant_id,owner_id,case_id,message_id,session_id,tool_call_id,tool,arguments,arguments_digest,mode,state,decided_by,decided_at,proposal_id) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,CASE WHEN $10='full' THEN 'allowed' ELSE 'pending' END,CASE WHEN $10='full' THEN 'full' END,CASE WHEN $10='full' THEN now() END,$11) ON CONFLICT(session_id,tool_call_id) DO NOTHING", &[&run.tenant,&run.owner,&run.case,&run.message,&session,&input.tool_call_id,&input.tool,&input.arguments,&fingerprint,&mode,&proposal]).await?;
    Ok(())
}
pub(crate) async fn by_call(tx: &Transaction<'_>, session: &str, call: &str) -> Result<Value> {
    object(tx,"SELECT to_jsonb(p) FROM agent_tool_permissions p WHERE session_id=$1 AND tool_call_id=$2 FOR UPDATE", &[&session,&call]).await
}
pub(crate) async fn mark_resumable(tx: &Transaction<'_>, mid: i64) -> Result<()> {
    tx.execute(
        "UPDATE agent_runs SET resumable=true WHERE message_id=$1",
        &[&mid],
    )
    .await?;
    Ok(())
}
pub(crate) async fn expire(tx: &Transaction<'_>, id: Uuid) -> Result<()> {
    tx.execute("UPDATE agent_tool_permissions SET state='denied',decided_by='expiry',reason='批准请求已过期',decided_at=now() WHERE id=$1", &[&id]).await?;
    Ok(())
}
pub(crate) async fn record_execution(
    tx: &Transaction<'_>,
    run: &ActiveRun,
    id: Uuid,
    result: &Value,
    audit: &Value,
) -> Result<()> {
    tx.execute("UPDATE agent_tool_permissions SET state='executed',result=$2,executed_at=now() WHERE id=$1", &[&id,result]).await?;
    tx.execute("INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) VALUES($1,$2,$3,'agent.action_executed',$4)", &[&run.tenant,&run.case,&run.owner,audit]).await?;
    Ok(())
}
