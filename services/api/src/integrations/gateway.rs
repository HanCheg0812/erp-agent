use super::{erp::erp, knowledge::knowledge, settings::Settings, types::Call};
use crate::{agent_sessions, platform::*};
use serde_json::{Value, json};

pub(super) async fn call(app: App, id: String, token: String, input: Call) -> Result<Value> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &id, &token, false).await?;
    let row = tx.query_opt("SELECT s.tenant_id,s.owner_id,s.case_id,c.customer_code,c.part_number,r.settings FROM agent_sessions s JOIN case_conversations cc ON cc.session_id=s.id JOIN cases c ON c.id=s.case_id JOIN agent_runs r ON r.case_id=c.id AND r.tenant_id=c.tenant_id WHERE s.id=$1 AND r.state='running' AND NOT r.cancel_requested", &[&id]).await?.ok_or_else(Error::forbidden)?;
    let settings: Settings = parse(row.get(5))?;
    settings.validate()?;
    let tenant: uuid::Uuid = row.get(0);
    let owner: uuid::Uuid = row.get(1);
    let case: uuid::Uuid = row.get(2);
    let customer: Option<String> = row.get(3);
    let part: Option<String> = row.get(4);
    let scopes: Vec<String> = tx.query("SELECT scope FROM integration_grants WHERE tenant_id=$1 AND owner_id=$2 AND domain=$3 AND (case_id IS NULL OR case_id=$4)", &[&tenant,&owner,&input.domain,&case]).await?.iter().map(|r| r.get(0)).collect();
    // Hold the session lease through the read. Revocation waits for this bounded call.
    let result = match (input.domain.as_str(), input.tool.as_str()) {
        ("erp", "list_erp_orders" | "read_erp_order") => {
            erp(&app, &input, scopes, customer, part).await?
        }
        ("openviking", "search_case_knowledge" | "read_knowledge_source") => {
            knowledge(&app, &input, scopes).await?
        }
        _ => return Err(Error::bad("Unknown domain tool")),
    };
    tx.execute("INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) VALUES($1,$2,$3,'agent.tool_read',$4)", &[&tenant,&case,&owner,&json!({"domain":input.domain,"tool":input.tool})]).await?;
    tx.commit().await?;
    Ok(result)
}
