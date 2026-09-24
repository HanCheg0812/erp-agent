use super::{
    credentials::{EnvironmentCredentialProvider, ErpCredentialProvider},
    erp::{self, ErpAuth, erp},
    knowledge::knowledge,
    settings::Settings,
    types::Call,
};
use crate::{agent_sessions, mcp_sessions, platform::*};
use axum::http::StatusCode;
use serde_json::{Value, json};

#[derive(Debug)]
struct ToolGrant {
    scope: String,
    tool_id: Option<String>,
    action: String,
    effect: String,
    active: bool,
}

fn legacy_tool(domain: &str, tool: &str) -> bool {
    matches!(
        (domain, tool),
        ("erp", "list_erp_orders" | "read_erp_order")
            | (
                "openviking",
                "search_case_knowledge" | "read_knowledge_source"
            )
    )
}

fn action_for(domain: &str, tool: &str) -> Result<&'static str> {
    if legacy_tool(domain, tool) {
        Ok("read")
    } else {
        Err(Error::bad("Unknown domain tool"))
    }
}

fn shared_demo_enabled_value(value: Option<&str>) -> bool {
    value == Some("true")
}

fn shared_demo_enabled() -> bool {
    shared_demo_enabled_value(std::env::var("ERP_SHARED_DEMO_MODE").ok().as_deref())
}

fn bound_user_auth_with_origin(
    erp_user_id: String,
    origin: String,
    credential_ref: Option<&str>,
) -> Result<ErpAuth> {
    let token = EnvironmentCredentialProvider.resolve(credential_ref)?;
    Ok(ErpAuth {
        token,
        origin,
        erp_user_id,
    })
}

fn authorize_tool(
    grants: Vec<ToolGrant>,
    domain: &str,
    tool: &str,
    action: &str,
) -> Result<Vec<String>> {
    let applicable: Vec<_> = grants
        .into_iter()
        .filter(|grant| {
            grant.active
                && grant.action == action
                && (grant.tool_id.as_deref() == Some(tool)
                    || grant.tool_id.is_none() && legacy_tool(domain, tool))
        })
        .collect();
    if applicable.iter().any(|grant| grant.effect == "deny") {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "platform_forbidden",
            "This integration tool is not permitted",
        ));
    }
    let scopes: Vec<_> = applicable
        .into_iter()
        .filter(|grant| grant.effect == "allow")
        .map(|grant| grant.scope)
        .collect();
    if scopes.is_empty() {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "platform_forbidden",
            "This integration tool is not permitted",
        ));
    }
    Ok(scopes)
}

async fn erp_auth(
    tx: &tokio_postgres::Transaction<'_>,
    tenant: uuid::Uuid,
    owner: uuid::Uuid,
) -> Result<ErpAuth> {
    let row = tx
        .query_opt(
            "SELECT c.base_url,c.auth_mode,c.active,b.erp_user_id,b.credential_ref,b.active FROM erp_identity_bindings b JOIN erp_connections c ON c.tenant_id=b.tenant_id AND c.id=b.connection_id WHERE b.tenant_id=$1 AND b.platform_user_id=$2 LIMIT 1",
            &[&tenant, &owner],
        )
        .await?;
    let Some(row) = row else {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "erp_identity_unbound",
            "No active ERPNext identity is bound to this platform user",
        ));
    };
    let base_url: String = row.get(0);
    let auth_mode: String = row.get(1);
    let connection_active: bool = row.get(2);
    let erp_user_id: String = row.get(3);
    let credential_ref: Option<String> = row.get(4);
    let binding_active: bool = row.get(5);
    if !connection_active || !binding_active {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "erp_identity_inactive",
            "The ERPNext identity binding is inactive",
        ));
    }
    if auth_mode == "shared_demo" {
        if !shared_demo_enabled() {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "erp_shared_identity_disabled",
                "Shared ERP identity mode is disabled",
            ));
        }
        if credential_ref.as_deref() != Some("env:ERP_API_TOKEN") {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "erp_credentials_unavailable",
                "The configured ERP credential reference is unavailable",
            ));
        }
        return shared_demo_auth_with_origin(erp_user_id, base_url);
    }
    bound_user_auth_with_origin(erp_user_id, base_url, credential_ref.as_deref())
}

fn shared_demo_auth_with_origin(erp_user_id: String, origin: String) -> Result<ErpAuth> {
    let token = std::env::var("ERP_API_TOKEN")
        .ok()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "erp_not_configured",
                "ERP integration credentials are not configured",
            )
        })?;
    Ok(ErpAuth {
        token,
        origin,
        erp_user_id,
    })
}

pub(super) async fn call(app: App, id: String, token: String, input: Call) -> Result<Value> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let is_mcp_session = id.starts_with("mcp_");
    let (tenant, owner, case, run_message_id, settings, customer, part) = if is_mcp_session {
        let (tenant, owner, case, run_message_id) =
            mcp_sessions::authorize(&tx, &id, &token).await?;
        let row = tx.query_opt(
            "SELECT c.customer_code,c.part_number,r.settings FROM mcp_sessions s LEFT JOIN cases c ON c.tenant_id=s.tenant_id AND c.id=s.case_id LEFT JOIN agent_runs r ON r.message_id=s.run_message_id WHERE s.id=$1 AND (s.run_message_id IS NULL OR (r.state='running' AND NOT r.cancel_requested))",
            &[&id],
        ).await?.ok_or_else(Error::forbidden)?;
        let settings: Settings = row
            .get::<_, Option<Value>>(2)
            .map(|value| parse(value))
            .transpose()?
            .unwrap_or_default();
        (
            tenant,
            owner,
            case,
            run_message_id,
            settings,
            row.get(0),
            row.get(1),
        )
    } else {
        agent_sessions::authorize(&tx, &id, &token, false).await?;
        let row = tx.query_opt("SELECT s.tenant_id,s.owner_id,s.case_id,c.customer_code,c.part_number,r.settings,r.message_id FROM agent_sessions s JOIN case_conversations cc ON cc.session_id=s.id JOIN cases c ON c.id=s.case_id JOIN agent_runs r ON r.case_id=c.id AND r.tenant_id=c.tenant_id WHERE s.id=$1 AND r.state='running' AND NOT r.cancel_requested", &[&id]).await?.ok_or_else(Error::forbidden)?;
        (
            row.get(0),
            row.get(1),
            Some(row.get(2)),
            Some(row.get(6)),
            parse(row.get(5))?,
            row.get(3),
            row.get(4),
        )
    };
    settings.validate()?;
    let action = action_for(&input.domain, &input.tool)?;
    let trace_id = uuid::Uuid::new_v4().to_string();
    let grants = if let Some(case_id) = case {
        tx.query("SELECT scope,tool_id,action,effect,active FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2 AND domain=$3 AND (case_id IS NULL OR case_id=$4)", &[&tenant,&owner,&input.domain,&case_id]).await?
    } else {
        tx.query("SELECT scope,tool_id,action,effect,active FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2 AND domain=$3 AND case_id IS NULL", &[&tenant,&owner,&input.domain]).await?
    };
    let grants = grants
        .into_iter()
        .map(|row| ToolGrant {
            scope: row.get(0),
            tool_id: row.get(1),
            action: row.get(2),
            effect: row.get(3),
            active: row.get(4),
        })
        .collect();
    let scopes = authorize_tool(grants, &input.domain, &input.tool, action)?;
    let erp_auth = if input.domain == "erp" {
        Some(erp_auth(&tx, tenant, owner).await?)
    } else {
        None
    };
    let erp_user = erp_auth.as_ref().map(|auth| auth.erp_user_id.clone());
    // Hold the session lease through the read. Revocation waits for this bounded call.
    let result = match (input.domain.as_str(), input.tool.as_str()) {
        ("erp", "list_erp_orders" | "read_erp_order") => {
            erp(
                &app,
                &input,
                scopes,
                customer,
                part,
                erp_auth.expect("ERP auth was resolved"),
            )
            .await?
        }
        ("openviking", "search_case_knowledge" | "read_knowledge_source") => {
            knowledge(&app, &input, scopes).await?
        }
        _ => unreachable!("tool was validated before dispatch"),
    };
    let result = if input.domain == "erp" {
        erp::result_envelope(&input.tool, result, &trace_id)?
    } else {
        result
    };
    tx.execute("INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) VALUES($1,$2,$3,'agent.tool_read',$4)", &[&tenant,&case,&owner,&json!({"domain":input.domain,"tool":input.tool,"erp_user":erp_user,"trace_id":trace_id,"mcp_session":is_mcp_session,"run_message_id":run_message_id})]).await?;
    tx.commit().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grant(scope: &str, tool_id: Option<&str>, effect: &str) -> ToolGrant {
        ToolGrant {
            scope: scope.into(),
            tool_id: tool_id.map(str::to_owned),
            action: "read".into(),
            effect: effect.into(),
            active: true,
        }
    }

    #[test]
    fn legacy_grants_only_cover_published_read_tools() {
        assert_eq!(
            authorize_tool(
                vec![grant("C1", None, "allow")],
                "erp",
                "list_erp_orders",
                "read"
            )
            .unwrap(),
            vec!["C1"]
        );
        assert!(
            authorize_tool(
                vec![grant("C1", None, "allow")],
                "erp",
                "future_write",
                "write"
            )
            .is_err()
        );
    }

    #[test]
    fn explicit_deny_wins_over_allow() {
        let result = authorize_tool(
            vec![
                grant("C1", Some("read_erp_order"), "allow"),
                grant("C1", Some("read_erp_order"), "deny"),
            ],
            "erp",
            "read_erp_order",
            "read",
        );
        assert!(matches!(
            result,
            Err(Error(StatusCode::FORBIDDEN, "platform_forbidden", _))
        ));
    }

    #[test]
    fn shared_demo_requires_explicit_true_flag() {
        assert!(shared_demo_enabled_value(Some("true")));
        assert!(!shared_demo_enabled_value(None));
        assert!(!shared_demo_enabled_value(Some("false")));
        assert!(!shared_demo_enabled_value(Some("TRUE")));
    }
}
