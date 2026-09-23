use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

pub(crate) struct Attachment {
    pub filename: String,
    pub kind: String,
    pub key: String,
    pub hash: String,
    pub size: i32,
}

pub(crate) async fn get<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    case_id: Uuid,
    document_id: Uuid,
) -> Result<Attachment> {
    let row = db.query_opt(
        "SELECT filename,content_type,object_key,sha256,bytes FROM documents WHERE id=$1 AND tenant_id=$2 AND owner_id=$3 AND case_id=$4 AND state='ready'",
        &[&document_id, &actor.tenant_id, &actor.user_id, &case_id],
    ).await?.ok_or_else(Error::missing)?;
    Ok(Attachment {
        filename: row.get(0),
        kind: row.get(1),
        key: row.get(2),
        hash: row.get(3),
        size: row.get(4),
    })
}

pub(crate) async fn audit(
    tx: &Transaction<'_>,
    actor: &Principal,
    case_id: Uuid,
    payload: &Value,
) -> Result<()> {
    tx.execute(
        "INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) VALUES($1,$2,$3,'agent.attachment_read',$4)",
        &[&actor.tenant_id, &case_id, &actor.user_id, payload],
    ).await?;
    Ok(())
}
