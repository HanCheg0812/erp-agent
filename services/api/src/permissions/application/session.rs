use super::super::repository;
use crate::agent_sessions;
use crate::{auth::Principal, cases, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn principal(
    tx: &tokio_postgres::Transaction<'_>,
    session: &str,
) -> Result<(Principal, Uuid)> {
    let (actor, case) = repository::session::principal(tx, session).await?;
    actor.require("planner")?;
    Ok((actor, case))
}
pub(crate) async fn read_case(app: App, session: String, token: String) -> Result<Value> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &session, &token, false).await?;
    let (actor, case) = principal(&tx, &session).await?;
    let mut record = cases::detail_in_tx(&*tx, &actor, case).await?;
    record["can_approve"] = json!(actor.roles.iter().any(|role| role == "approver"));
    let settings = repository::session::settings(&*tx, &actor, case)
        .await?
        .unwrap_or_else(|| json!({"approval":"ask"}));
    record["authorization"] = json!({
        "mode": settings["approval"].as_str().unwrap_or("ask"),
        "operation_tools": ["memory_save", "case_set_context", "proposal_create"],
        "read_tools": [
            "case_read", "attachment_read", "todo", "skill_find", "skill_manage",
            "mcp__erp__list_erp_orders", "mcp__erp__read_erp_order",
            "mcp__openviking__search_case_knowledge", "mcp__openviking__read_knowledge_source"
        ],
        "business_approval_tool": "request_proposal_approval",
        "can_approve_business_proposals": actor.roles.iter().any(|role| role == "approver"),
        "business_approval_requires_human": true
    });
    tx.commit().await?;
    Ok(record)
}
