use crate::{auth::Principal, platform::*};
use serde_json::Value;
use uuid::Uuid;

pub(crate) async fn lock_approval_key(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    key: &str,
) -> Result<()> {
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        &[&format!("{}:{}:{key}", actor.tenant_id, actor.user_id)],
    )
    .await?;
    Ok(())
}

pub(crate) async fn approval_by_key(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    key: &str,
) -> Result<Option<Value>> {
    Ok(tx.query_opt("SELECT to_jsonb(a) FROM approvals a WHERE tenant_id=$1 AND actor_id=$2 AND idempotency_key=$3", &[&actor.tenant_id, &actor.user_id, &key]).await?.map(|row| row.get(0)))
}

pub(crate) async fn proposal_digest(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    proposal: Uuid,
    version: i32,
) -> Result<Option<String>> {
    Ok(tx.query_opt("SELECT digest FROM proposals WHERE tenant_id=$1 AND case_id=$2 AND id=$3 AND case_version=$4", &[&actor.tenant_id, &id, &proposal, &version]).await?.map(|row| row.get(0)))
}

pub(crate) async fn insert_approval(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    input: &super::super::types::ApprovalInput,
    key: &str,
    hash: &str,
) -> Result<Value> {
    object(tx, "INSERT INTO approvals(tenant_id,case_id,proposal_id,proposal_digest,decision,comment,actor_id,idempotency_key,request_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) RETURNING to_jsonb(approvals)", &[&actor.tenant_id, &id, &input.proposal_id, &input.proposal_digest, &input.decision, &input.comment, &actor.user_id, &key, &hash]).await
}

pub(crate) async fn mark_decision(
    tx: &tokio_postgres::Transaction<'_>,
    id: Uuid,
    decision: &str,
) -> Result<()> {
    tx.execute(
        "UPDATE cases SET version=version+1,status=$2,updated_at=now() WHERE id=$1",
        &[&id, &decision],
    )
    .await?;
    Ok(())
}

pub(crate) async fn complete_approval_requests(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    input: &super::super::types::ApprovalInput,
    result: &Value,
) -> Result<()> {
    // Preserve the run-before-permission lock order used by cancellation.
    tx.query("SELECT message_id FROM agent_runs WHERE case_id=$1 AND state IN ('running','waiting') FOR UPDATE", &[&id]).await?;
    tx.execute("UPDATE agent_tool_permissions SET state='executed',decided_by='user',reason=$2,result=$3,decided_at=now(),executed_at=now() WHERE proposal_id=$1 AND tenant_id=$4 AND owner_id=$5 AND state='pending'", &[&input.proposal_id, &input.decision, &result, &actor.tenant_id, &actor.user_id]).await?;
    tx.execute("UPDATE agent_runs r SET state='queued',updated_at=now() WHERE r.case_id=$1 AND r.state='waiting' AND NOT r.cancel_requested AND EXISTS(SELECT 1 FROM agent_tool_permissions p WHERE p.message_id=r.message_id AND p.proposal_id=$2 AND p.state='executed') AND NOT EXISTS(SELECT 1 FROM agent_tool_permissions p WHERE p.message_id=r.message_id AND p.state='pending')", &[&id, &input.proposal_id]).await?;
    Ok(())
}
