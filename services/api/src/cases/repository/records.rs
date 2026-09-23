use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::GenericClient;
use uuid::Uuid;

pub(crate) async fn find<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    lock: bool,
) -> Result<Value> {
    object(
        db,
        if lock {
            "SELECT to_jsonb(c) FROM cases c WHERE tenant_id=$1 AND owner_id=$2 AND id=$3 FOR UPDATE"
        } else {
            "SELECT to_jsonb(c) FROM cases c WHERE tenant_id=$1 AND owner_id=$2 AND id=$3"
        },
        &[&actor.tenant_id, &actor.user_id, &id],
    )
    .await
}

pub(crate) async fn insert_conversation(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    request: Uuid,
) -> Result<Option<Value>> {
    Ok(tx.query_opt("INSERT INTO cases(tenant_id,owner_id,title,kind,quantity_basis,conversation_request_id) VALUES($1,$2,'New conversation','conversation','unknown',$3) ON CONFLICT (tenant_id,owner_id,conversation_request_id) DO NOTHING RETURNING to_jsonb(cases)", &[&actor.tenant_id, &actor.user_id, &request]).await?.map(|row| row.get(0)))
}

pub(crate) async fn conversation_by_request(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    request: Uuid,
) -> Result<Value> {
    object(tx, "SELECT to_jsonb(c) FROM cases c WHERE tenant_id=$1 AND owner_id=$2 AND conversation_request_id=$3", &[&actor.tenant_id, &actor.user_id, &request]).await
}

pub(crate) async fn insert_case(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    input: &super::super::types::CaseInput,
) -> Result<Value> {
    object(tx, "INSERT INTO cases(tenant_id,owner_id,title,customer_code,part_number,revision,demand_type,quantity_basis) VALUES($1,$2,$3,$4,$5,$6,$7,$8) RETURNING to_jsonb(cases)", &[&actor.tenant_id, &actor.user_id, &input.title, &input.customer_code, &input.part_number, &input.revision, &input.demand_type, &input.quantity_basis]).await
}

pub(crate) async fn has_active_run(
    tx: &tokio_postgres::Transaction<'_>,
    actor: &Principal,
    id: Uuid,
) -> Result<bool> {
    Ok(tx.query_opt("SELECT 1 FROM agent_runs WHERE tenant_id=$1 AND case_id=$2 AND state IN ('queued','running','waiting')", &[&actor.tenant_id, &id]).await?.is_some())
}

pub(crate) async fn update_context(
    tx: &tokio_postgres::Transaction<'_>,
    id: Uuid,
    input: &super::super::types::CaseInput,
    version: i32,
) -> Result<Value> {
    object(tx, "UPDATE cases SET kind='case',title=$2,customer_code=$3,part_number=$4,revision=$5,demand_type=$6,quantity_basis=$7,version=$8,updated_at=now() WHERE id=$1 RETURNING to_jsonb(cases)", &[&id, &input.title, &input.customer_code, &input.part_number, &input.revision, &input.demand_type, &input.quantity_basis, &version]).await
}
