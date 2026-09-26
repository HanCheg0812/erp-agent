use super::{
    provider::{call, configured, matches, preflight, remote_id},
    reconciliation::reconcile,
    repository::{ROW, save},
};
use crate::{agent_sessions, cases, mcp_sessions};
use crate::{auth::Principal, platform::*};
use axum::http::StatusCode;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::net::{IpAddr, Ipv4Addr};
use uuid::Uuid;

fn add_internal_allow_out(values: &mut Vec<String>, host: &str) {
    let host = host.trim();
    if host.is_empty() {
        return;
    }
    let value = host
        .parse::<IpAddr>()
        .map(|ip| format!("{ip}/32"))
        .unwrap_or_else(|_| host.to_owned());
    if !values.iter().any(|current| current == &value) {
        values.push(value);
    }
}

fn add_dns_resolver_allow_out(values: &mut Vec<String>, resolver: &str) -> Result<()> {
    let resolver = resolver.parse::<Ipv4Addr>().map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "CUBE_DNS_RESOLVER must be an IPv4 address",
        )
    })?;
    add_internal_allow_out(values, &resolver.to_string());
    Ok(())
}

pub(super) fn cube_allow_out() -> Result<Vec<String>> {
    let mut values = env("CUBE_ALLOW_OUT", "100.64.0.1/32,api.deepseek.com")
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();

    // The resident guest must be able to call the Rust internal listener for
    // PostgreSQL-backed session checkpoints and MCP receipts. Keep this
    // address in the policy even when an older worker inherited the fallback
    // allow-list and did not load CUBE_ALLOW_OUT from the runtime overlay.
    if let Ok(host) = std::env::var("API_INTERNAL_HOST") {
        add_internal_allow_out(&mut values, &host);
    }
    // Cube v0.7.2 does not preserve its implicit DNS exception after pause/resume.
    add_dns_resolver_allow_out(&mut values, &env("CUBE_DNS_RESOLVER", "169.254.254.53"))?;
    Ok(values)
}

fn resident_envs(
    app: &App,
    operation: Uuid,
    session: &str,
    storage_token: &str,
    mcp_session: &str,
    mcp_token: &str,
    settings: &crate::integrations::Settings,
) -> Result<Value> {
    let origin = std::env::var("AGENT_STORAGE_ORIGIN").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "AGENT_STORAGE_ORIGIN is required for Cube runtime",
        )
    })?;
    let model_key = app.model_key.as_deref().ok_or_else(|| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "Dedicated model credential is not configured",
        )
    })?;
    let runtime_token = super::bridge::token_for(operation)?;
    let storage_http_origin = reqwest::Url::parse(&origin)
        .ok()
        .filter(|url| url.scheme() == "http")
        .map(|url| url.origin().ascii_serialization());
    let mut values = BTreeMap::<String, String>::from([
        // Template warmup is only for the image registration request. A normal
        // instance must perform the full session and credential readiness check.
        ("CUBE_TEMPLATE_WARMUP".into(), "0".to_owned()),
        ("AGENT_RUNTIME_MODE".into(), "resident".to_owned()),
        (
            "AGENT_RUNTIME_BIND".into(),
            format!("0.0.0.0:{}", super::bridge::port()?),
        ),
        ("AGENT_RUNTIME_AUTH_TOKEN".into(), runtime_token),
        (
            "AGENT_MCP_BINARY".into(),
            "/usr/local/bin/erp-agent-mcp".to_owned(),
        ),
        (
            "AGENT_TURN_TIMEOUT_SECONDS".into(),
            env("AGENT_TURN_TIMEOUT_SECONDS", "0"),
        ),
        (
            "AGENT_COMPACTION_SOFT_RATIO".into(),
            env("AGENT_COMPACTION_SOFT_RATIO", "0.80"),
        ),
        (
            "AGENT_COMPACTION_CRITICAL_RATIO".into(),
            env("AGENT_COMPACTION_CRITICAL_RATIO", "0.90"),
        ),
        ("OPENAI_API_KEY".into(), model_key.to_owned()),
        ("JCODE_OPENAI_MODEL".into(), settings.model.clone()),
        (
            "JCODE_OPENAI_API_BASE".into(),
            env("DEEPSEEK_BASE_URL", "https://api.deepseek.com"),
        ),
        ("JCODE_OPENAI_TRANSPORT".into(), "https".to_owned()),
        (
            "JCODE_OPENAI_NATIVE_COMPACTION_MODE".into(),
            "off".to_owned(),
        ),
        ("JCODE_OPENAI_MAX_OUTPUT_TOKENS".into(), "4096".to_owned()),
        ("JCODE_MAX_RETRIES".into(), "1".to_owned()),
        ("JCODE_MEMORY_ENABLED".into(), "false".to_owned()),
        ("JCODE_MEMORY_SIDECAR_ENABLED".into(), "false".to_owned()),
        ("JCODE_NO_TELEMETRY".into(), "1".to_owned()),
        ("DO_NOT_TRACK".into(), "1".to_owned()),
        ("JCODE_PRESERVE_REASONING_CONTEXT".into(), "true".to_owned()),
        ("JCODE_SESSION_STORE_URL".into(), origin),
        ("JCODE_SESSION_STORE_ID".into(), session.to_owned()),
        ("JCODE_SESSION_STORE_TOKEN".into(), storage_token.to_owned()),
        ("JCODE_MCP_SESSION_ID".into(), mcp_session.to_owned()),
        ("JCODE_MCP_SESSION_TOKEN".into(), mcp_token.to_owned()),
    ]);
    if let Some(origin) = storage_http_origin {
        values.insert("JCODE_SESSION_STORE_ALLOW_HTTP_ORIGIN".into(), origin);
    }
    values.insert(
        "ERP_SHARED_DEMO_MODE".into(),
        env("ERP_SHARED_DEMO_MODE", "0"),
    );
    if let Ok(value) = std::env::var("ERP_API_TOKEN") {
        values.insert("ERP_API_TOKEN".into(), value);
    }
    if let Ok(value) = std::env::var("OPENVIKING_URL") {
        values.insert("OPENVIKING_URL".into(), value);
    }
    if let Ok(value) = std::env::var("OPENVIKING_API_KEY") {
        values.insert("OPENVIKING_API_KEY".into(), value);
    }
    for (key, value) in std::env::vars()
        .filter(|(key, value)| key.starts_with("ERP_CREDENTIAL_") && !value.trim().is_empty())
    {
        values.insert(key, value);
    }
    Ok(json!(values))
}

async fn configure_guest(
    app: &App,
    sandbox_id: &str,
    operation: Uuid,
    env_vars: &Value,
) -> Result<()> {
    let url = super::bridge::guest_url(sandbox_id, "/v1/configure")?;
    let token = super::bridge::token_for(operation)?;
    let timeout = env("CUBE_RUNTIME_READY_TIMEOUT_SECONDS", "60")
        .parse::<u64>()
        .map_err(|_| Error::bad("Invalid Cube runtime ready timeout"))?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    let body = json!({"env": env_vars});
    let mut attempts = 0u32;
    loop {
        attempts = attempts.saturating_add(1);
        let result = app
            .http
            .post(&url)
            .header("x-agent-runtime-token", &token)
            .timeout(std::time::Duration::from_secs(3))
            .json(&body)
            .send()
            .await;
        match result {
            Ok(response) if response.status().is_success() => {
                eprintln!(
                    "Cube guest configuration accepted: sandbox={sandbox_id} attempts={attempts}"
                );
                return Ok(());
            }
            Ok(response) if response.status() == StatusCode::CONFLICT => {
                // A retry after the API has already completed configuration is
                // safe only when the guest proves it is out of template warmup.
                let probe = app
                    .http
                    .get(super::bridge::guest_url(sandbox_id, "/health/ready")?)
                    .timeout(std::time::Duration::from_secs(2))
                    .send()
                    .await;
                if let Ok(probe) = probe {
                    if probe.status().is_success() {
                        if let Ok(value) = probe.json::<Value>().await {
                            if value["status"] == "ready" && value["warmup"] != true {
                                return Ok(());
                            }
                        }
                    }
                }
                eprintln!(
                    "Cube guest configuration conflict: sandbox={sandbox_id} attempts={attempts}"
                );
                return Err(Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "cube_runtime_not_ready",
                    "Cube resident configuration was rejected",
                ));
            }
            Ok(response)
                if matches!(
                    response.status(),
                    StatusCode::NOT_FOUND
                        | StatusCode::BAD_GATEWAY
                        | StatusCode::SERVICE_UNAVAILABLE
                        | StatusCode::GATEWAY_TIMEOUT
                ) =>
            {
                eprintln!(
                    "Cube guest configuration retry: sandbox={sandbox_id} attempts={attempts} status={}",
                    response.status()
                );
            }
            Ok(_) => {
                eprintln!(
                    "Cube guest configuration rejected: sandbox={sandbox_id} attempts={attempts}"
                );
                return Err(Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "cube_runtime_not_ready",
                    "Cube resident configuration failed",
                ));
            }
            Err(error) => {
                eprintln!(
                    "Cube guest configuration transport retry: sandbox={sandbox_id} attempts={attempts} error={error}"
                );
            }
        }
        if std::time::Instant::now() >= deadline {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cube_runtime_not_ready",
                "Cube resident runtime did not accept its session configuration in time",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }
}

pub(crate) async fn ensure_resident_ready(
    app: &App,
    actor: &Principal,
    case_id: Uuid,
) -> Result<()> {
    let ready_timeout = env("CUBE_RUNTIME_READY_TIMEOUT_SECONDS", "60")
        .parse::<u64>()
        .map_err(|_| Error::bad("Invalid Cube runtime ready timeout"))?;
    if !(5..=300).contains(&ready_timeout) {
        return Err(Error::bad(
            "Cube runtime ready timeout must be between 5 and 300 seconds",
        ));
    }

    let started = std::time::Instant::now();
    let mut create_attempted = false;
    loop {
        let status = operate(app, actor, case_id, "status", None).await?;
        let operation = status.get("operation").and_then(Value::as_object);
        if operation.is_some_and(|value| {
            value.get("state").and_then(Value::as_str) == Some("provisioned")
                && value.get("guest_ready").and_then(Value::as_bool) == Some(true)
        }) {
            return Ok(());
        }

        let active = operation.is_some_and(|value| {
            !matches!(
                value.get("state").and_then(Value::as_str),
                None | Some("released")
            )
        });
        if !active && !create_attempted {
            actor.require("planner")?;
            operate(app, actor, case_id, "create", Some(Uuid::new_v4())).await?;
            create_attempted = true;
        }

        if started.elapsed() >= std::time::Duration::from_secs(ready_timeout) {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cube_runtime_not_ready",
                "Cube resident runtime did not become ready in time",
            ));
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
}
fn view(mut row: Value) -> Value {
    if let Some(o) = row.as_object_mut() {
        for key in [
            "tenant_id",
            "case_id",
            "actor_id",
            "mcp_session_id",
            "expired",
        ] {
            o.remove(key);
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_cube_allow_out_uses_a_host_cidr_and_deduplicates() {
        let mut values = vec!["100.64.0.1/32".to_owned()];
        add_internal_allow_out(&mut values, "192.168.100.122");
        add_internal_allow_out(&mut values, "192.168.100.122");
        assert_eq!(values, ["100.64.0.1/32", "192.168.100.122/32"]);
    }

    #[test]
    fn internal_cube_allow_out_keeps_hostnames_without_cidr_rewriting() {
        let mut values = Vec::new();
        add_internal_allow_out(&mut values, "api.internal.example");
        assert_eq!(values, ["api.internal.example"]);
    }

    #[test]
    fn cube_dns_resolver_is_a_deduplicated_ipv4_cidr() {
        let mut values = vec!["169.254.254.53/32".to_owned()];
        add_dns_resolver_allow_out(&mut values, "169.254.254.53").unwrap();
        assert_eq!(values, ["169.254.254.53/32"]);
        assert!(add_dns_resolver_allow_out(&mut values, "dns.example.test").is_err());
    }

    #[test]
    fn public_sandbox_view_removes_internal_scope_and_expiry_fields() {
        let value = view(json!({
            "id":"operation","tenant_id":"tenant","case_id":"case","actor_id":"user",
            "mcp_session_id":"mcp_secret","expired":false,"state":"provisioned","sandbox_id":"cube-1"
        }));
        for key in [
            "tenant_id",
            "case_id",
            "actor_id",
            "mcp_session_id",
            "expired",
        ] {
            assert!(value.get(key).is_none(), "{key}");
        }
        assert_eq!(value["state"], "provisioned");
        assert_eq!(value["sandbox_id"], "cube-1");
    }
}
pub(super) async fn operate(
    app: &App,
    actor: &Principal,
    id: Uuid,
    action: &str,
    operation: Option<Uuid>,
) -> Result<Value> {
    let mut db = app.db.get().await?;
    cases::case(&**db, actor, id, false).await?;
    let key = format!("erp-cube:{}:{id}", actor.tenant_id);
    let locked: bool = db
        .query_one(
            "SELECT pg_try_advisory_lock(hashtextextended($1,0))",
            &[&key],
        )
        .await?
        .get(0);
    if !locked {
        return Err(Error::conflict("sandbox_busy"));
    }
    let result = operate_locked(app, &mut db, actor, id, action, operation).await;
    if db
        .query_one("SELECT pg_advisory_unlock(hashtextextended($1,0))", &[&key])
        .await
        .is_err()
    {
        drop(deadpool_postgres::Object::take(db));
    }
    result
}
async fn operate_locked(
    app: &App,
    db: &mut deadpool_postgres::Object,
    actor: &Principal,
    id: Uuid,
    action: &str,
    operation: Option<Uuid>,
) -> Result<Value> {
    if action != "status" {
        actor.require("planner")?;
    }
    let current=db.query_opt(&format!("SELECT {ROW} FROM case_sandboxes s WHERE tenant_id=$1 AND case_id=$2 ORDER BY created_at DESC,id DESC LIMIT 1"),&[&actor.tenant_id,&id]).await?.map(|r|r.get::<_,Value>(0));
    let row = if action == "create" {
        if !configured() {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cube_not_configured",
                "Runtime provider is not configured",
            ));
        }
        let operation = operation.unwrap();
        if let Some(prior) = db
            .query_opt(
                &format!("SELECT {ROW} FROM case_sandboxes s WHERE id=$1"),
                &[&operation],
            )
            .await?
        {
            let prior: Value = prior.get(0);
            if prior["tenant_id"] != actor.tenant_id.to_string()
                || prior["case_id"] != id.to_string()
                || prior["actor_id"] != actor.user_id.to_string()
            {
                return Err(Error::conflict("idempotency_conflict"));
            }
            Some(reconcile(app, db, actor, prior).await?)
        } else {
            if let Some(current) = current {
                if reconcile(app, db, actor, current).await?["state"] != "released" {
                    return Err(Error::conflict("sandbox_exists"));
                }
            }
            preflight(app, &env("CUBE_TEMPLATE_ID", "")).await?;
            let tx = db.transaction().await?;
            cases::case(&*tx, actor, id, true).await?;
            let session = agent_sessions::bind(&tx, actor, id).await?;
            let mut mcp_session_id: Option<String> = None;
            let mut env_vars = None;
            if super::bridge::enabled() {
                let claim = agent_sessions::claim(&*tx, actor, &session).await?;
                let settings: crate::integrations::Settings = serde_json::from_value(
                    tx.query_one(
                        "SELECT settings FROM case_conversations WHERE tenant_id=$1 AND case_id=$2",
                        &[&actor.tenant_id, &id],
                    )
                    .await?
                    .get(0),
                )
                .map_err(|_| Error::bad("Invalid conversation settings"))?;
                settings.validate()?;
                let (mcp_id, mcp_token) = mcp_sessions::insert(&*tx, actor, Some(id), None).await?;
                env_vars = Some(resident_envs(
                    app,
                    operation,
                    &session,
                    claim["storage_token"].as_str().unwrap(),
                    &mcp_id,
                    &mcp_token,
                    &settings,
                )?);
                mcp_session_id = Some(mcp_id);
            }
            let mut row=object(&*tx,&format!("INSERT INTO case_sandboxes AS s(id,tenant_id,case_id,session_id,actor_id,mcp_session_id,state) VALUES($1,$2,$3,$4,$5,$6,'creating') RETURNING {ROW}"),&[&operation,&actor.tenant_id,&id,&session,&actor.user_id,&mcp_session_id]).await?;
            cases::audit(
                &*tx,
                actor,
                id,
                "sandbox.creating",
                json!({"operation_id":operation,"session_id":session}),
            )
            .await?;
            tx.commit().await?;
            let metadata = json!({"project":"erp-agent","erp_operation":operation,"erp_case":id,"erp_tenant":actor.tenant_id,"erp_user":actor.user_id,"erp_session":session});
            let allow_out = cube_allow_out()?;
            let create_request = json!({
                "templateID":env("CUBE_TEMPLATE_ID",""),
                "timeout":900,
                "allow_internet_access":false,
                "metadata":metadata,
                "network":{"allow_out":allow_out}
            });
            let runtime_envs = env_vars;
            if let Ok(created) = call(
                app,
                "/sandboxes",
                reqwest::Method::POST,
                Some(create_request),
            )
            .await
            {
                let sandbox_id = remote_id(&created)?;
                // Cube's create acknowledgement contains the ID but no metadata.
                // Read the instance before binding its ID and credentials.
                if let Ok(remote) = call(
                    app,
                    &format!("/sandboxes/{sandbox_id}"),
                    reqwest::Method::GET,
                    None,
                )
                .await
                {
                    if !matches(&remote, &row) {
                        return Err(Error::conflict("sandbox_scope_mismatch"));
                    }
                    row = save(db, actor, &row, json!({"sandbox_id":sandbox_id})).await?;
                }
            }
            row = reconcile(app, db, actor, row).await?;
            if let (Some(env_vars), Some(sandbox_id)) =
                (runtime_envs.as_ref(), row["sandbox_id"].as_str())
            {
                if row["state"] != "released" && row["error_code"] != "scope_mismatch" {
                    configure_guest(app, sandbox_id, operation, env_vars).await?;
                    row = reconcile(app, db, actor, row).await?;
                }
            }
            Some(row)
        }
    } else if action == "release" {
        let mut row=object(&***db,&format!("SELECT {ROW} FROM case_sandboxes s WHERE id=$1 AND tenant_id=$2 AND case_id=$3 AND actor_id=$4"),&[&operation.unwrap(),&actor.tenant_id,&id,&actor.user_id]).await?;
        if row["state"] != "released" {
            row = save(
                db,
                actor,
                &row,
                json!({"state":"releasing","guest_ready":false}),
            )
            .await?
        }
        Some(reconcile(app, db, actor, row).await?)
    } else {
        match current {
            Some(r) => Some(reconcile(app, db, actor, r).await?),
            None => None,
        }
    };
    Ok(
        json!({"configured":configured(),"can_manage":actor.roles.iter().any(|r|r=="planner"),"operation":row.map(view)}),
    )
}
