use crate::cases;
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
pub(super) const ROW: &str = "to_jsonb(s)||jsonb_build_object('expired',expires_at<=now())";
pub(super) async fn save(
    db: &mut deadpool_postgres::Object,
    actor: &Principal,
    row: &Value,
    patch: Value,
) -> Result<Value> {
    let mut next = row.clone();
    for (k, v) in patch.as_object().unwrap() {
        next[k] = v.clone()
    }
    // A released operation is historical bookkeeping only. Do not keep the
    // provider id reserved, because Cube can assign that id to a later
    // sandbox and `sandbox_id` is globally unique in PostgreSQL.
    if next["state"] == "released" {
        next["sandbox_id"] = Value::Null;
    }
    let tx = db.transaction().await?;
    let id = uuid(row["id"].as_str().unwrap())?;
    let saved=object(&*tx,&format!("UPDATE case_sandboxes s SET sandbox_id=$2,state=$3,provider_state=$4,guest_ready=$5,error_code=$6,mcp_session_id=$7,checked_at=now() WHERE id=$1 RETURNING {ROW}"),&[&id,&next["sandbox_id"].as_str(),&next["state"].as_str().unwrap(),&next["provider_state"].as_str(),&next["guest_ready"].as_bool().unwrap_or(false),&next["error_code"].as_str(),&next["mcp_session_id"].as_str()]).await?;
    if saved["state"] != row["state"] {
        cases::audit(
            &*tx,
            actor,
            uuid(row["case_id"].as_str().unwrap())?,
            &format!("sandbox.{}", saved["state"].as_str().unwrap()),
            json!({"operation_id":id,"sandbox_id":saved["sandbox_id"]}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(saved)
}
