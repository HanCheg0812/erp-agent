use super::records::find;
use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::GenericClient;
use uuid::Uuid;

pub(crate) async fn detail<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Value> {
    let mut record = find(db, actor, id, false).await?;
    record["proposals"] = serde_json::json!(array(
        db,
        "SELECT to_jsonb(p) FROM proposals p WHERE tenant_id=$1 AND case_id=$2 ORDER BY case_version DESC",
        &[&actor.tenant_id, &id],
    )
    .await?);
    record["approvals"] = serde_json::json!(array(
        db,
        "SELECT to_jsonb(a)-'request_digest'-'idempotency_key' FROM approvals a WHERE tenant_id=$1 AND case_id=$2 ORDER BY created_at DESC",
        &[&actor.tenant_id, &id],
    )
    .await?);
    record["documents"] = serde_json::json!(array(
        db,
        "SELECT to_jsonb(d)-'object_key'-'tenant_id'-'owner_id' FROM documents d WHERE tenant_id=$1 AND owner_id=$2 AND case_id=$3 ORDER BY created_at DESC LIMIT 100",
        &[&actor.tenant_id, &actor.user_id, &id],
    )
    .await?);
    Ok(record)
}

pub(crate) async fn list<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    limit: i64,
) -> Result<Vec<Value>> {
    array(db, "SELECT to_jsonb(c) FROM cases c WHERE tenant_id=$1 AND owner_id=$2 ORDER BY created_at DESC,id DESC LIMIT $3", &[&actor.tenant_id, &actor.user_id, &limit]).await
}

pub(crate) async fn events<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    after: i64,
    limit: i64,
) -> Result<Vec<Value>> {
    array(db, "SELECT to_jsonb(e)||jsonb_build_object('id',id::text) FROM audit_events e WHERE tenant_id=$1 AND case_id=$2 AND id>$3 ORDER BY id LIMIT $4", &[&actor.tenant_id, &id, &after, &limit]).await
}
