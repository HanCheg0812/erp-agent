use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

pub(crate) async fn list<C: GenericClient + Sync>(db: &C, actor: &Principal) -> Result<Vec<Value>> {
    array(db,"SELECT jsonb_build_object('id',id,'text',text,'created_at',created_at) FROM user_memories WHERE tenant_id=$1 AND owner_id=$2 ORDER BY created_at DESC,id LIMIT 50",&[&actor.tenant_id,&actor.user_id]).await
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
            "SELECT count(*) FROM user_memories WHERE tenant_id=$1 AND owner_id=$2",
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
) -> Result<Value> {
    object(tx,"INSERT INTO user_memories(id,tenant_id,owner_id,text) VALUES($1,$2,$3,$4) RETURNING jsonb_build_object('id',id,'text',text,'created_at',created_at)",&[&id,&actor.tenant_id,&actor.user_id,&text]).await
}
pub(crate) async fn delete<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    id: Uuid,
) -> Result<bool> {
    Ok(db
        .execute(
            "DELETE FROM user_memories WHERE id=$1 AND tenant_id=$2 AND owner_id=$3",
            &[&id, &actor.tenant_id, &actor.user_id],
        )
        .await?
        != 0)
}
