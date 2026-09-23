use crate::{auth::Principal, platform::*};
use serde_json::Value;
use uuid::Uuid;

pub(crate) async fn evidence_is_ready(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    document: Uuid,
) -> Result<bool> {
    Ok(tx.query_opt("SELECT 1 FROM documents WHERE id=$1 AND tenant_id=$2 AND owner_id=$3 AND case_id=$4 AND state='ready'", &[&document, &actor.tenant_id, &actor.user_id, &id]).await?.is_some())
}

pub(crate) async fn insert_proposal(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    input: &super::super::types::ProposalInput,
    version: i32,
    hash: &str,
) -> Result<Value> {
    object(tx, "INSERT INTO proposals(tenant_id,case_id,case_version,author_id,summary,actions,evidence,digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING to_jsonb(proposals)", &[&actor.tenant_id, &id, &version, &actor.user_id, &input.summary, &serde_json::json!(input.actions), &serde_json::json!(input.evidence), &hash]).await
}

pub(crate) async fn mark_awaiting_approval(
    tx: &tokio_postgres::Transaction<'_>,
    id: Uuid,
    version: i32,
) -> Result<()> {
    tx.execute(
        "UPDATE cases SET version=$2,status='awaiting_approval',updated_at=now() WHERE id=$1",
        &[&id, &version],
    )
    .await?;
    Ok(())
}

pub(crate) async fn expire_proposal_requests(
    tx: &tokio_postgres::Transaction<'_>,
    id: Uuid,
) -> Result<()> {
    tx.execute("UPDATE agent_tool_permissions SET state='denied',reason='提案版本已更新，请重新提交审批',decided_by='expiry',decided_at=now() WHERE case_id=$1 AND tool='request_proposal_approval' AND state='pending'", &[&id]).await?;
    Ok(())
}
