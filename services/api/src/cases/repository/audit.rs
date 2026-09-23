use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::GenericClient;
use uuid::Uuid;

pub(crate) async fn audit<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    event: &str,
    payload: Value,
) -> Result<()> {
    db.execute(
        "INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) VALUES($1,$2,$3,$4,$5)",
        &[&actor.tenant_id, &id, &actor.user_id, &event, &payload],
    )
    .await?;
    Ok(())
}
