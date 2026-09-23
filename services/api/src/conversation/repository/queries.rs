use crate::{auth::Principal, platform::*};
use serde_json::Value;
use uuid::Uuid;

pub(crate) async fn session<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Option<String>> {
    Ok(db
        .query_opt(
            "SELECT session_id FROM case_conversations WHERE tenant_id=$1 AND case_id=$2",
            &[&actor.tenant_id, &id],
        )
        .await?
        .map(|r| r.get(0)))
}

pub(crate) async fn settings<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(db
        .query_opt(
            "SELECT settings FROM case_conversations WHERE tenant_id=$1 AND case_id=$2",
            &[&actor.tenant_id, &id],
        )
        .await?
        .map(|r| r.get(0)))
}

pub(crate) async fn history<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    before: Option<i64>,
    limit: i64,
) -> Result<Vec<Value>> {
    array(db, "SELECT to_jsonb(m)||jsonb_build_object('id',m.id::text,'operation',COALESCE(r.operation,'message'),'response',CASE WHEN r.message_id IS NULL THEN NULL ELSE jsonb_build_object('state',r.state,'text',r.text,'tools',r.tools,'steps',r.steps,'compactions',r.compactions,'error',r.error) END) FROM case_conversation_messages m LEFT JOIN agent_runs r ON r.message_id=m.id WHERE m.tenant_id=$1 AND m.case_id=$2 AND ($3::bigint IS NULL OR m.id<$3) ORDER BY m.id DESC LIMIT $4", &[&actor.tenant_id, &id, &before, &limit]).await
}

pub(crate) async fn context_usage<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Option<Value>> {
    Ok(db
        .query_opt(
            "SELECT context_usage FROM agent_runs WHERE tenant_id=$1 AND case_id=$2 AND context_usage IS NOT NULL ORDER BY updated_at DESC LIMIT 1",
            &[&actor.tenant_id, &id],
        )
        .await?
        .map(|row| row.get(0)))
}

pub(crate) async fn permissions<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    mid: i64,
    can_approve: bool,
) -> Result<Vec<Value>> {
    array(db, "SELECT jsonb_build_object('id',p.id,'tool_call_id',p.tool_call_id,'tool',p.tool,'arguments',p.arguments,'arguments_digest',p.arguments_digest,'mode',p.mode,'state',p.state,'decided_by',p.decided_by,'reason',p.reason,'created_at',p.created_at,'expires_at',p.expires_at,'result',p.result,'proposal',to_jsonb(b),'can_approve',$4::boolean) FROM agent_tool_permissions p LEFT JOIN proposals b ON b.id=p.proposal_id AND b.tenant_id=p.tenant_id AND b.case_id=p.case_id WHERE p.message_id=$1 AND p.tenant_id=$2 AND p.owner_id=$3 ORDER BY p.created_at", &[&mid, &actor.tenant_id, &actor.user_id, &can_approve]).await
}
