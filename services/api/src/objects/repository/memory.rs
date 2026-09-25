use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

pub(crate) async fn list<C: GenericClient + Sync>(db: &C, actor: &Principal) -> Result<Vec<Value>> {
    array(db,"SELECT jsonb_build_object('id',id,'memory_id',memory_id,'version',version,'text',text,'category',category,'status',status,'created_at',created_at,'updated_at',updated_at) FROM user_memories WHERE tenant_id=$1 AND owner_id=$2 AND scope_kind='user' AND COALESCE(scope_id,owner_id)=$2 AND status='active' AND (valid_to IS NULL OR valid_to>now()) ORDER BY created_at DESC,id LIMIT 50",&[&actor.tenant_id,&actor.user_id]).await
}
pub(crate) async fn recall<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    query: &str,
) -> Result<Vec<Value>> {
    array(db,"SELECT jsonb_build_object('id',id,'memory_id',memory_id,'version',version,'text',text,'category',category,'status',status,'created_at',created_at,'updated_at',updated_at) FROM user_memories WHERE tenant_id=$1 AND owner_id=$2 AND scope_kind='user' AND COALESCE(scope_id,owner_id)=$2 AND status='active' AND (valid_to IS NULL OR valid_to>now()) ORDER BY CASE WHEN btrim($3)<>'' AND normalized_text=lower(btrim($3)) THEN 3 WHEN btrim($3)<>'' AND position(lower(btrim($3)) in lower(text))>0 THEN 2 WHEN btrim($3)<>'' AND search_vector @@ plainto_tsquery('simple',$3) THEN 1 ELSE 0 END DESC, created_at DESC,id DESC LIMIT 12",&[&actor.tenant_id,&actor.user_id,&query]).await
}
pub(crate) async fn lock_owner(tx: &Transaction<'_>, actor: &Principal) -> Result<()> {
    tx.query_one(
        "SELECT pg_advisory_xact_lock(hashtextextended($1,0))",
        &[&format!("memory:{}", actor.user_id)],
    )
    .await?;
    Ok(())
}
pub(crate) async fn by_request(
    tx: &Transaction<'_>,
    id: Uuid,
) -> Result<Option<(Uuid, Uuid, String)>> {
    Ok(tx
        .query_opt(
            "SELECT tenant_id,owner_id,text FROM user_memories WHERE id=$1",
            &[&id],
        )
        .await?
        .map(|row| (row.get(0), row.get(1), row.get(2))))
}
pub(crate) async fn count(tx: &Transaction<'_>, actor: &Principal) -> Result<i64> {
    Ok(tx
        .query_one(
            "SELECT count(*) FROM user_memories WHERE tenant_id=$1 AND owner_id=$2 AND scope_kind='user' AND COALESCE(scope_id,owner_id)=$2 AND status='active' AND (valid_to IS NULL OR valid_to>now())",
            &[&actor.tenant_id, &actor.user_id],
        )
        .await?
        .get(0))
}
pub(crate) async fn insert(
    tx: &Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    text: &str,
    source_kind: &str,
) -> Result<Value> {
    let source_hash = digest(text);
    let value = object(tx,"INSERT INTO user_memories(id,tenant_id,owner_id,text,memory_id,scope_kind,scope_id,category,status,source_kind,source_id,source_hash,approved_by,approved_at,approval_hash) VALUES($1,$2,$3,$4,$1,'user',$3,'preference','active',$5,$1,$6,$3,now(),$6) RETURNING jsonb_build_object('id',id,'memory_id',memory_id,'version',version,'text',text,'category',category,'status',status,'created_at',created_at,'updated_at',updated_at)",&[&id,&actor.tenant_id,&actor.user_id,&text,&source_kind,&source_hash]).await?;
    if semantic_memory_embeddings_enabled() {
        tx.execute("INSERT INTO memory_outbox(tenant_id,owner_id,memory_version_id,operation) VALUES($1,$2,$3,'embed') ON CONFLICT (memory_version_id,operation) DO NOTHING", &[&actor.tenant_id,&actor.user_id,&id]).await?;
    }
    tx.execute("INSERT INTO memory_audit_events(tenant_id,owner_id,memory_version_id,event_type,payload) VALUES($1,$2,$3,'memory.created',jsonb_build_object('source_kind',$4::text,'category','preference','scope_kind','user'))", &[&actor.tenant_id,&actor.user_id,&id,&source_kind]).await?;
    Ok(value)
}
pub(crate) async fn retract(tx: &Transaction<'_>, actor: &Principal, id: Uuid) -> Result<bool> {
    lock_owner(tx, actor).await?;
    let changed = tx
        .execute(
            "UPDATE user_memories SET status='retracted',valid_to=COALESCE(valid_to,now()),updated_at=now() WHERE id=$1 AND tenant_id=$2 AND owner_id=$3 AND status='active'",
            &[&id, &actor.tenant_id, &actor.user_id],
        )
        .await?;
    if changed != 0 {
        if semantic_memory_embeddings_enabled() {
            tx.execute("INSERT INTO memory_outbox(tenant_id,owner_id,memory_version_id,operation) VALUES($1,$2,$3,'retract') ON CONFLICT (memory_version_id,operation) DO NOTHING", &[&actor.tenant_id,&actor.user_id,&id]).await?;
        }
        tx.execute("INSERT INTO memory_audit_events(tenant_id,owner_id,memory_version_id,event_type,payload) VALUES($1,$2,$3,'memory.retracted',jsonb_build_object('reason','user_request'))", &[&actor.tenant_id,&actor.user_id,&id]).await?;
    }
    Ok(changed != 0)
}

fn semantic_memory_embeddings_enabled() -> bool {
    matches!(
        std::env::var("PERSONAL_MEMORY_EMBEDDINGS_ENABLED")
            .ok()
            .as_deref(),
        Some("1" | "true" | "on")
    )
}
