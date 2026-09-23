use crate::platform::*;
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;
pub(super) async fn retry(
    tx: &Transaction<'_>,
    id: &str,
    request: Uuid,
    hash: &str,
) -> Result<Option<Value>> {
    if let Some(r)=tx.query_opt("SELECT digest,result FROM agent_session_receipts WHERE session_id=$1 AND request_id=$2",&[&id,&request]).await?{
        if r.get::<_,String>(0)!=hash{return Err(Error::conflict("idempotency_conflict"));}return Ok(Some(r.get(1)));
    }
    Ok(None)
}
pub(super) async fn receipt(
    tx: &Transaction<'_>,
    id: &str,
    request: Uuid,
    hash: &str,
    revision: i32,
) -> Result<Value> {
    let value = json!({"revision":revision});
    tx.execute("INSERT INTO agent_session_receipts(session_id,request_id,digest,result) VALUES($1,$2,$3,$4)",&[&id,&request,&hash,&value]).await?;
    Ok(value)
}
