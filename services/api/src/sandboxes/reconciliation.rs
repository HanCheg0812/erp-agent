use super::{
    provider::{call, configured, matches, remote_id, unavailable},
    repository::{ROW, save},
};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
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
        row = save(db, actor, &row, json!({"sandbox_id":id})).await?;
    }
    let path = format!("/sandboxes/{}", row["sandbox_id"].as_str().unwrap());
    let remote = call(app, &path, reqwest::Method::GET, None).await?;
    if remote.is_null() {
        return save(
            db,
            actor,
            &row,
            json!({"state":"released","provider_state":null,"guest_ready":false,"error_code":null}),
        )
        .await;
    }
    if !matches(&remote, &row) {
        return save(
            db,
            actor,
            &row,
            json!({"guest_ready":false,"error_code":"scope_mismatch"}),
        )
        .await;
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
        return save(
            db,
            actor,
            &row,
            json!({"state":"released","provider_state":null,"guest_ready":false,"error_code":null}),
        )
        .await;
    }
    let mut healthy = false;
    if let Ok(proxy) = std::env::var("CUBE_PROXY_URL") {
        let url = format!(
            "{}/sandbox/{}/49999/health",
            proxy.trim_end_matches('/'),
            row["sandbox_id"].as_str().unwrap()
        );
        if let Ok(r) = app
            .http
            .get(url)
            .timeout(std::time::Duration::from_secs(3))
            .send()
            .await
        {
            if r.status().is_success() {
                healthy = r.json::<Value>().await.is_ok_and(|v| v == "OK")
            }
        }
    }
    save(db,actor,&row,json!({"state":"provisioned","provider_state":remote["state"],"guest_ready":healthy,"error_code":if healthy{Value::Null}else{json!("guest_unavailable")}})).await
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
                json!({"error_code":"cube_unavailable","guest_ready":false}),
            )
            .await
        }
    }
}
