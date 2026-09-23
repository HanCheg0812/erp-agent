use crate::platform::*;
use crate::{
    auth::{self, Principal},
    cases,
};
use serde_json::{Value, json};
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

fn new_session_id() -> String {
    format!("session_{}", Uuid::new_v4().simple())
}

pub async fn create(app: App, actor: Principal, id: Uuid) -> Result<Value> {
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, &actor, id, true).await?;
    let session = new_session_id();
    tx.execute(
        "INSERT INTO agent_sessions(id,tenant_id,case_id,owner_id) VALUES($1,$2,$3,$4)",
        &[&session, &actor.tenant_id, &id, &actor.user_id],
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"id":session,"case_id":id}))
}
pub async fn bind(tx: &Transaction<'_>, actor: &Principal, id: Uuid) -> Result<String> {
    if let Some(row) = tx
        .query_opt(
            "SELECT session_id FROM case_conversations WHERE tenant_id=$1 AND case_id=$2",
            &[&actor.tenant_id, &id],
        )
        .await?
    {
        return Ok(row.get(0));
    }
    let session = new_session_id();
    tx.execute(
        "INSERT INTO agent_sessions(id,tenant_id,case_id,owner_id) VALUES($1,$2,$3,$4)",
        &[&session, &actor.tenant_id, &id, &actor.user_id],
    )
    .await?;
    tx.execute(
        "INSERT INTO case_conversations(tenant_id,case_id,session_id) VALUES($1,$2,$3)",
        &[&actor.tenant_id, &id, &session],
    )
    .await?;
    Ok(session)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_session_ids_satisfy_the_public_validator() {
        let first = new_session_id();
        let second = new_session_id();
        assert_ne!(first, second);
        assert!(session_id(&first).is_ok());
        assert_eq!(first.len(), 40);
    }
}
pub async fn claim<C: GenericClient + Sync>(db: &C, actor: &Principal, id: &str) -> Result<Value> {
    actor.require("planner")?;
    session_id(id)?;
    let token = auth::token();
    let row=db.query_opt("UPDATE agent_sessions s SET token_hash=$1,fence=fence+1,expires_at=now()+interval '8 hours' FROM cases c WHERE s.id=$2 AND s.tenant_id=$3 AND s.owner_id=$4 AND c.id=s.case_id AND c.tenant_id=s.tenant_id AND c.owner_id=s.owner_id RETURNING jsonb_build_object('id',s.id,'fence',s.fence,'expires_at',s.expires_at,'revision',s.revision)",&[&digest(&token),&id,&actor.tenant_id,&actor.user_id]).await?.ok_or_else(Error::missing)?;
    let mut value: Value = row.get(0);
    value["storage_token"] = json!(token);
    Ok(value)
}
pub async fn claim_route(app: App, actor: Principal, id: String) -> Result<Value> {
    let db = app.db.get().await?;
    claim(&**db, &actor, &id).await
}
