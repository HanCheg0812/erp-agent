use super::{
    lifecycle::cube_allow_out,
    provider::{call, configured, matches, remote_id, unavailable},
    repository::{ROW, save},
};
use crate::{agent_sessions, auth::Principal, platform::*};
use serde_json::{Value, json};

async fn release_leases(db: &tokio_postgres::Client, row: &Value) -> Result<()> {
    let session = row["session_id"].as_str().unwrap_or_default();
    if let Some(lease) = row["session_lease_id"].as_str() {
        if !session.is_empty() {
            agent_sessions::revoke_runtime_lease(db, uuid(lease)?, session).await?;
        }
    } else if row["active"] != false && !session.is_empty() {
        agent_sessions::revoke_base(db, session).await?;
    }
    if let Some(mcp) = row["mcp_session_id"].as_str() {
        db.execute(
            "UPDATE mcp_sessions SET revoked_at=now(),updated_at=now() WHERE id=$1 AND revoked_at IS NULL",
            &[&mcp],
        )
        .await?;
    }
    Ok(())
}
async fn reconcile_inner(
    app: &App,
    db: &mut deadpool_postgres::Object,
    actor: &Principal,
    mut row: Value,
) -> Result<Value> {
    if !configured() || row["state"] == "released" {
        return Ok(row);
    }
    if row["sandbox_id"].is_null() {
        let found = call(
            app,
            &format!(
                "/sandboxes?metadata=erp_operation%3D{}&limit=1000",
                row["id"].as_str().unwrap()
            ),
            reqwest::Method::GET,
            None,
        )
        .await?;
        let found = found
            .as_array()
            .ok_or_else(unavailable)?
            .iter()
            .filter(|r| r["metadata"]["erp_operation"] == row["id"])
            .collect::<Vec<_>>();
        if found.is_empty() && (row["state"] == "releasing" || row["expired"] == true) {
            release_leases(&***db, &row).await?;
            return save(
                db,
                actor,
                &row,
                json!({"state":"released","provider_state":null,"guest_ready":false,"error_code":null}),
            )
            .await;
        }
        if found.len() != 1 || !matches(found[0], &row) {
            return save(
                db,
                actor,
                &row,
                json!({"error_code":"creation_unconfirmed","guest_ready":false}),
            )
            .await;
        }
        let id = remote_id(found[0])?;
        let patch = if super::bridge::enabled() {
            json!({"sandbox_id":id,"error_code":"configuration_pending","guest_ready":false})
        } else {
            json!({"sandbox_id":id})
        };
        row = save(db, actor, &row, patch).await?;
    }
    let path = format!("/sandboxes/{}", row["sandbox_id"].as_str().unwrap());
    let mut remote = call(app, &path, reqwest::Method::GET, None).await?;
    if remote.is_null() {
        release_leases(&***db, &row).await?;
        return save(
            db,
            actor,
            &row,
            json!({"state":"released","provider_state":null,"guest_ready":false,"error_code":null}),
        )
        .await;
    }
    let paused = remote["state"]
        .as_str()
        .is_some_and(|state| state.eq_ignore_ascii_case("paused"));
    let mut reconnect_pending = row["error_code"] == "resume_reconnect_pending";
    // The provider scope was strictly checked before the sandbox ID was
    // persisted. Cube v0.7.2 can omit metadata on an already-bound sandbox
    // while it transitions through pause/resume, so only a supplied metadata
    // object can contradict the persisted scope here.
    if remote["metadata"].is_object() && !matches(&remote, &row) {
        return save(
            db,
            actor,
            &row,
            json!({"guest_ready":false,"error_code":"scope_mismatch"}),
        )
        .await;
    }
    if row["error_code"] == "configuration_pending"
        && row["state"] != "releasing"
        && row["expired"] != true
    {
        return Ok(row);
    }
    // Cube drops guest-initiated outbound sockets on pause. Persist the reset
    // requirement before resuming so a retry can rebuild JCode's connections.
    if paused {
        row = save(
            db,
            actor,
            &row,
            json!({"guest_ready":false,"error_code":"resume_reconnect_pending"}),
        )
        .await?;
        reconnect_pending = true;
        call(
            app,
            &format!("{path}/resume"),
            reqwest::Method::POST,
            Some(json!({})),
        )
        .await?;
        remote = call(app, &path, reqwest::Method::GET, None).await?;
        // Cube may omit metadata while a paused guest transitions back to
        // running. The sandbox ID was scope-checked immediately before the
        // resume request, so only reject a response that supplies and
        // contradicts metadata; a transiently absent object is acceptable.
        if remote.is_null() || (remote["metadata"].is_object() && !matches(&remote, &row)) {
            return save(
                db,
                actor,
                &row,
                json!({"guest_ready":false,"error_code":"scope_mismatch"}),
            )
            .await;
        }
    }
    if row["state"] == "releasing" || row["expired"] == true {
        row = save(
            db,
            actor,
            &row,
            json!({"state":"releasing","guest_ready":false}),
        )
        .await?;
        call(app, &path, reqwest::Method::DELETE, None).await?;
        if !call(app, &path, reqwest::Method::GET, None)
            .await?
            .is_null()
        {
            return save(db, actor, &row, json!({"error_code":"release_pending"})).await;
        }
        release_leases(&***db, &row).await?;
        return save(
            db,
            actor,
            &row,
            json!({"state":"released","provider_state":null,"guest_ready":false,"error_code":null}),
        )
        .await;
    }
    if reconnect_pending {
        call(
            app,
            &format!("{path}/network"),
            reqwest::Method::PUT,
            Some(json!({"allowInternetAccess":false,"allowOut":cube_allow_out()?})),
        )
        .await?;
    }
    let mut healthy = false;
    if std::env::var("CUBE_PROXY_URL").is_ok() {
        let resident = super::bridge::enabled();
        let route = if resident { "/health/ready" } else { "/health" };
        if let Ok(url) = super::bridge::guest_url(row["sandbox_id"].as_str().unwrap(), route) {
            if let Ok(r) = app
                .http
                .get(url)
                .timeout(std::time::Duration::from_secs(3))
                .send()
                .await
            {
                if r.status().is_success() {
                    healthy = r.json::<Value>().await.is_ok_and(|v| {
                        if resident {
                            v["status"] == "ready" && v["warmup"] != true
                        } else {
                            v == "OK"
                        }
                    });
                }
            }
        }
    }
    if reconnect_pending && healthy {
        let url = super::bridge::guest_url(row["sandbox_id"].as_str().unwrap(), "/v1/reconnect")?;
        let token = super::bridge::token_for(uuid(row["id"].as_str().unwrap())?)?;
        healthy = app
            .http
            .post(url)
            .header("x-agent-runtime-token", token)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
            .is_ok_and(|response| response.status().is_success());
    }
    let error_code = if healthy {
        Value::Null
    } else if reconnect_pending {
        json!("resume_reconnect_pending")
    } else {
        json!("guest_unavailable")
    };
    save(db,actor,&row,json!({"state":"provisioned","provider_state":remote["state"],"guest_ready":healthy,"error_code":error_code})).await
}
pub(super) async fn reconcile(
    app: &App,
    db: &mut deadpool_postgres::Object,
    actor: &Principal,
    row: Value,
) -> Result<Value> {
    match reconcile_inner(app, db, actor, row.clone()).await {
        Ok(v) => Ok(v),
        Err(_) => {
            // Reconciliation may have durably discovered an ID or begun deletion.
            let id = uuid(row["id"].as_str().unwrap())?;
            let latest = object(
                &***db,
                &format!("SELECT {ROW} FROM case_sandboxes s WHERE id=$1"),
                &[&id],
            )
            .await?;
            save(
                db,
                actor,
                &latest,
                json!({"error_code":if latest["error_code"] == "resume_reconnect_pending" {"resume_reconnect_pending"} else {"cube_unavailable"},"guest_ready":false}),
            )
            .await
        }
    }
}
