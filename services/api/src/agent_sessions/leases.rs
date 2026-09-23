use crate::platform::*;
use serde_json::Value;
use tokio_postgres::Transaction;
pub(crate) async fn authorize(
    tx: &Transaction<'_>,
    id: &str,
    token: &str,
    write: bool,
) -> Result<(i32, i32, Option<Value>)> {
    session_id(id)?;
    let query = format!(
        "SELECT s.revision,s.fence,s.metadata FROM agent_sessions s JOIN users u ON u.id=s.owner_id AND u.tenant_id=s.tenant_id JOIN cases c ON c.id=s.case_id AND c.tenant_id=s.tenant_id AND c.owner_id=s.owner_id WHERE s.id=$1 AND s.token_hash=$2 AND s.expires_at>now() AND u.active AND 'planner'=ANY(u.roles) FOR {} OF s",
        if write { "UPDATE" } else { "SHARE" }
    );
    let row = tx
        .query_opt(&query, &[&id, &digest(token)])
        .await?
        .ok_or_else(Error::unauthorized)?;
    Ok((row.get(0), row.get(1), row.get(2)))
}
