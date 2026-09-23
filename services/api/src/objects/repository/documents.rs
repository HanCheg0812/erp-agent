use super::super::domain::UploadDocument;
use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;
const VIEW: &str = "to_jsonb(d)-'object_key'-'tenant_id'-'owner_id'";
pub(crate) async fn list<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Vec<Value>> {
    array(db,&format!("SELECT {VIEW} FROM documents d WHERE tenant_id=$1 AND owner_id=$2 AND case_id=$3 ORDER BY created_at DESC LIMIT 100"),&[&actor.tenant_id,&actor.user_id,&id]).await
}
pub(crate) async fn by_request(tx: &Transaction<'_>, id: Uuid) -> Result<Option<Value>> {
    Ok(tx
        .query_opt("SELECT to_jsonb(d) FROM documents d WHERE id=$1", &[&id])
        .await?
        .map(|r| r.get(0)))
}
pub(crate) async fn insert_upload(
    tx: &Transaction<'_>,
    actor: &Principal,
    case: Uuid,
    key: &str,
    input: &UploadDocument,
) -> Result<()> {
    tx.execute("INSERT INTO documents(id,tenant_id,owner_id,case_id,object_key,filename,content_type,bytes,sha256,state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'uploading')",&[&input.request_id,&actor.tenant_id,&actor.user_id,&case,&key,&input.filename,&input.content_type,&(input.data.len() as i32),&input.hash]).await?;
    Ok(())
}
pub(crate) async fn mark_failed<C: GenericClient + Sync>(db: &C, id: Uuid) -> Result<()> {
    db.execute(
        "UPDATE documents SET state='failed' WHERE id=$1 AND state<>'ready'",
        &[&id],
    )
    .await?;
    Ok(())
}
pub(crate) async fn mark_ready<C: GenericClient + Sync>(db: &C, id: Uuid) -> Result<Value> {
    object(
        db,
        &format!("UPDATE documents d SET state='ready' WHERE id=$1 RETURNING {VIEW}"),
        &[&id],
    )
    .await
}
pub(crate) async fn ready<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<Value> {
    object(db,"SELECT to_jsonb(d) FROM documents d JOIN cases c ON c.id=d.case_id AND c.tenant_id=d.tenant_id AND c.owner_id=d.owner_id WHERE d.id=$1 AND d.tenant_id=$2 AND d.owner_id=$3 AND d.state='ready'",&[&id,&actor.tenant_id,&actor.user_id]).await
}
