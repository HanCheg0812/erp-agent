use crate::{auth, auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_postgres::{GenericClient, Transaction};
use uuid::Uuid;

fn new_id() -> String {
    format!("mcp_{}", Uuid::new_v4().simple())
}

fn valid_id(id: &str) -> Result<()> {
    if id.len() == 36
        && id.starts_with("mcp_")
        && id[4..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Ok(())
    } else {
        Err(Error::bad("Invalid MCP session identifier"))
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub case_id: Option<Uuid>,
}

pub async fn create(app: App, actor: Principal, input: CreateInput) -> Result<Value> {
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    if let Some(case_id) = input.case_id {
        crate::cases::case(&*tx, &actor, case_id, true).await?;
    }
    let (id, token) = insert(&*tx, &actor, input.case_id, None).await?;
    tx.commit().await?;
    Ok(json!({"id": id, "case_id": input.case_id, "capability": token}))
}

pub async fn insert<C: GenericClient + Sync>(
    db: &C,
    actor: &Principal,
    case_id: Option<Uuid>,
    run_message_id: Option<i64>,
) -> Result<(String, String)> {
    if let Some(run_message_id) = run_message_id {
        db.execute(
            "UPDATE mcp_sessions SET revoked_at=now(),updated_at=now() WHERE run_message_id=$1 AND revoked_at IS NULL",
            &[&run_message_id],
        )
        .await?;
    }
    let id = new_id();
    let token = auth::token();
    db.execute(
        "INSERT INTO mcp_sessions(id,tenant_id,owner_id,case_id,run_message_id,token_hash,expires_at) VALUES($1,$2,$3,$4,$5,$6,CASE WHEN $4::uuid IS NOT NULL AND $5::bigint IS NULL THEN now()+interval '8 hours' ELSE now()+interval '15 minutes' END)",
        &[&id, &actor.tenant_id, &actor.user_id, &case_id, &run_message_id, &digest(&token)],
    ).await?;
    Ok((id, token))
}

pub async fn revoke(app: App, actor: Principal, id: String) -> Result<Value> {
    actor.require("planner")?;
    valid_id(&id)?;
    let db = app.db.get().await?;
    let count = db.execute(
        "UPDATE mcp_sessions SET revoked_at=now(),updated_at=now() WHERE id=$1 AND tenant_id=$2 AND owner_id=$3 AND revoked_at IS NULL",
        &[&id, &actor.tenant_id, &actor.user_id],
    ).await?;
    if count == 0 {
        return Err(Error::missing());
    }
    Ok(json!({"id": id, "revoked": true}))
}

pub(crate) async fn authorize(
    tx: &Transaction<'_>,
    id: &str,
    token: &str,
) -> Result<(Uuid, Uuid, Option<Uuid>, Option<i64>)> {
    valid_id(id)?;
    let row = tx.query_opt(
        "SELECT s.tenant_id,s.owner_id,s.case_id,s.run_message_id FROM mcp_sessions s JOIN users u ON u.tenant_id=s.tenant_id AND u.id=s.owner_id WHERE s.id=$1 AND s.token_hash=$2 AND s.expires_at>now() AND s.revoked_at IS NULL AND u.active AND 'planner'=ANY(u.roles) FOR UPDATE OF s",
        &[&id, &digest(token)],
    ).await?.ok_or_else(Error::unauthorized)?;
    Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
}

pub async fn create_route(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Json(input): Json<CreateInput>,
) -> Result<(axum::http::StatusCode, Json<Value>)> {
    Ok((
        axum::http::StatusCode::CREATED,
        Json(create(app, actor, input).await?),
    ))
}

pub async fn revoke_route(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(revoke(app, actor, id).await?))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_distinct_and_scoped() {
        let id = new_id();
        assert!(valid_id(&id).is_ok());
        assert!(valid_id("session_012345678901234567890123456789012345").is_err());
    }
}
