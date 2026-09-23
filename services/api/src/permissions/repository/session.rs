use crate::{auth::Principal, platform::*};
use serde_json::Value;
use tokio_postgres::GenericClient;
use uuid::Uuid;
pub(crate) async fn principal(
    tx: &tokio_postgres::Transaction<'_>,
    session: &str,
) -> Result<(Principal, Uuid)> {
    let row = tx.query_opt("SELECT u.tenant_id,u.id,u.roles,u.email,s.case_id FROM agent_sessions s JOIN users u ON u.tenant_id=s.tenant_id AND u.id=s.owner_id WHERE s.id=$1 AND u.active", &[&session]).await?.ok_or_else(Error::forbidden)?;
    let actor = Principal {
        tenant_id: row.get(0),
        user_id: row.get(1),
        roles: row.get(2),
        email: row.get(3),
    };
    Ok((actor, row.get(4)))
}
pub(crate) async fn settings<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    case: Uuid,
) -> Result<Option<Value>> {
    Ok(db
        .query_opt(
            "SELECT settings FROM case_conversations WHERE tenant_id=$1 AND case_id=$2",
            &[&actor.tenant_id, &case],
        )
        .await?
        .map(|r| r.get(0)))
}
