use super::super::{
    domain, repository,
    types::{Action, Memory, ProposalRequest},
};
use super::session::principal;
use crate::agent_sessions;
use crate::{cases, platform::*};
use serde_json::{Value, json};
// An approval belongs to the original call and exact parameters. Model text
// cannot grant approval or select the account/case being modified.
pub(crate) async fn call(app: App, session: String, token: String, input: Action) -> Result<Value> {
    domain::validate_action(&input)?;
    let fingerprint = json_digest(&input.arguments);
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &session, &token, false).await?;
    let (actor, case_id) = principal(&tx, &session).await?;
    let current = cases::case(&*tx, &actor, case_id, true).await?;
    let run = repository::actions::active_run(&tx, &session).await?;
    let case = run.case;
    let mid = run.message;
    let settings: crate::integrations::Settings = parse(run.settings.clone())?;
    settings.validate()?;
    let mut proposal_id = None;
    let business_request = input.tool == "request_proposal_approval";
    if business_request {
        let request: ProposalRequest = parse(input.arguments.clone())?;
        proposal_id = Some(request.proposal_id);
        if !repository::actions::exists(&tx, &session, &input.tool_call_id).await? {
            let matches = repository::actions::proposal_matches(&tx, &run, &request).await?;
            domain::check_proposal_request(&current, &request, matches)?;
        }
    }
    let mode = domain::permission_mode(&input.tool, &settings.approval);
    repository::actions::insert(&tx, &run, &session, &input, &fingerprint, mode, proposal_id)
        .await?;
    let mut permission = repository::actions::by_call(&tx, &session, &input.tool_call_id).await?;
    domain::check_call_binding(&permission, &input, &fingerprint, mid)?;
    repository::actions::mark_resumable(&tx, mid).await?;
    let pid = uuid(permission["id"].as_str().unwrap())?;
    let expired = repository::decisions::expired(&*tx, pid).await?;
    if domain::should_expire(&permission, expired) {
        repository::actions::expire(&tx, pid).await?;
        permission["state"] = json!("denied");
        permission["reason"] = json!("批准请求已过期");
    }
    let result = match permission["state"].as_str().unwrap_or("") {
        "pending" => json!({"status":"pending","permission_id":pid}),
        "executed" => permission["result"].clone(),
        "allowed" => {
            let result = match input.tool.as_str() {
                "case_set_context" => {
                    json!({"status":"saved","case":cases::set_context_tx(&tx,&actor,case,input.arguments.clone(),true).await?})
                }
                "proposal_create" => {
                    json!({"status":"created","proposal":cases::propose_tx(&tx,&actor,case,input.arguments.clone()).await?,"erp_executed":false})
                }
                "memory_save" => {
                    let memory: Memory = parse(input.arguments.clone())?;
                    crate::objects::save_agent_memory_tx(&tx, &actor, pid, &memory.text).await?
                }
                _ => return Err(Error::forbidden()),
            };
            repository::actions::record_execution(&tx, &run, pid, &result,
                &json!({"permission_id":pid,"tool":input.tool,"arguments_digest":fingerprint,"mode":settings.approval})).await?;
            result
        }
        _ => json!({"status":"denied","reason":permission["reason"]}),
    };
    tx.commit().await?;
    Ok(result)
}
