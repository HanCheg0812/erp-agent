use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

pub(crate) async fn existing_message<C: tokio_postgres::GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
    request: Uuid,
) -> Result<Option<(Value, String)>> {
    Ok(db.query_opt("SELECT to_jsonb(m)||jsonb_build_object('id',m.id::text),COALESCE(r.operation,'message') FROM case_conversation_messages m LEFT JOIN agent_runs r ON r.message_id=m.id WHERE m.tenant_id=$1 AND m.case_id=$2 AND m.client_message_id=$3", &[&actor.tenant_id, &id, &request]).await?.map(|row| (row.get(0), row.get(1))))
}

pub(crate) async fn insert_message(
    tx: &Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    request: Uuid,
    text: &str,
    settings: &Value,
) -> Result<Value> {
    object(tx, "INSERT INTO case_conversation_messages(tenant_id,case_id,actor_id,client_message_id,text,settings) VALUES($1,$2,$3,$4,$5,$6) RETURNING to_jsonb(case_conversation_messages)||jsonb_build_object('id',id::text)", &[&actor.tenant_id, &id, &actor.user_id, &request, &text, settings]).await
}

pub(crate) async fn update_title(
    tx: &Transaction<'_>,
    id: Uuid,
    title: &str,
    message_id: i64,
) -> Result<()> {
    tx.execute("UPDATE cases SET title=$2,updated_at=now() WHERE id=$1 AND kind='conversation' AND NOT EXISTS (SELECT 1 FROM case_conversation_messages WHERE case_id=$1 AND id<>$3)", &[&id, &title, &message_id]).await?;
    Ok(())
}

pub(crate) async fn attachments<C: GenericClient + Sync>(db: &C, message_id: i64) -> Result<Value> {
    let rows = db.query("SELECT jsonb_build_object('id',d.id,'filename',d.filename,'content_type',d.content_type,'bytes',d.bytes,'sha256',a.source_sha256,'state',d.state) FROM conversation_message_attachments a JOIN documents d ON d.id=a.document_id AND d.tenant_id=a.tenant_id AND d.owner_id=a.owner_id AND d.case_id=a.case_id WHERE a.message_id=$1 ORDER BY a.ordinal", &[&message_id]).await?;
    Ok(json!(
        rows.into_iter()
            .map(|r| r.get::<_, Value>(0))
            .collect::<Vec<_>>()
    ))
}

pub(crate) async fn attach(
    tx: &Transaction<'_>,
    actor: &Principal,
    case_id: Uuid,
    message_id: i64,
    ids: &[Uuid],
) -> Result<Value> {
    for (ordinal, document_id) in ids.iter().enumerate() {
        let inserted = tx.execute("INSERT INTO conversation_message_attachments(tenant_id,owner_id,case_id,message_id,document_id,ordinal,source_sha256) SELECT d.tenant_id,d.owner_id,d.case_id,$5,d.id,$6,d.sha256 FROM documents d WHERE d.id=$1 AND d.tenant_id=$2 AND d.owner_id=$3 AND d.case_id=$4 AND d.state='ready'", &[document_id,&actor.tenant_id,&actor.user_id,&case_id,&message_id,&(ordinal as i16)]).await?;
        if inserted != 1 {
            return Err(Error::bad("Attachment is unavailable for this case"));
        }
    }
    attachments(tx, message_id).await
}
