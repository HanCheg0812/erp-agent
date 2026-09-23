use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn update_settings(
    tx: &Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    value: &Value,
) -> Result<()> {
    tx.execute(
        "UPDATE case_conversations SET settings=$3 WHERE tenant_id=$1 AND case_id=$2",
        &[&actor.tenant_id, &id, value],
    )
    .await?;
    Ok(())
}
