use super::super::repository;
use crate::integrations::{self, Settings};
use crate::{
    auth::Principal,
    cases::{self, Page},
    platform::*,
};
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn history(app: App, actor: Principal, id: Uuid, page: Page) -> Result<Value> {
    let limit = page.limit()?;
    let before = page
        .before
        .map(|s| s.parse::<i64>().map_err(|_| Error::bad("Invalid cursor")))
        .transpose()?;
    if before.is_some_and(|b| b < 1) {
        return Err(Error::bad("Invalid cursor"));
    }
    let mut db = app.db.get().await?;
    let tx = db
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let case = cases::case(&*tx, &actor, id, false).await?;
    let session = repository::session(&*tx, &actor, id).await?;
    let settings: Settings = repository::settings(&*tx, &actor, id)
        .await?
        .and_then(|value: Value| {
            let parsed: Result<Settings> = parse(value);
            parsed
                .ok()
                .and_then(|settings| settings.validate().ok().map(|_| settings))
        })
        .unwrap_or_default();
    let previous_context = repository::context_usage(&*tx, &actor, id).await?;
    let used_tokens = previous_context
        .as_ref()
        .and_then(|value| value["used_tokens"].as_u64())
        .unwrap_or(0);
    let compacting = previous_context
        .as_ref()
        .and_then(|value| value["compacting"].as_bool())
        .unwrap_or(false);
    let mut context_usage = integrations::context_usage(&settings.model, used_tokens, compacting);
    context_usage["measured"] = json!(previous_context.is_some() || session.is_none());
    if let Some(window) = previous_context
        .as_ref()
        .filter(|value| value["model"] == settings.model)
        .and_then(|value| value["window_tokens"].as_u64())
        .filter(|window| *window > 0)
    {
        context_usage["window_tokens"] = json!(window);
    }
    let mut items = repository::history(&*tx, &actor, id, before, limit + 1).await?;
    let more = items.len() > limit as usize;
    items.truncate(limit as usize);
    items.reverse();
    for item in &mut items {
        let mid: i64 = item["id"].as_str().unwrap().parse().unwrap();
        item["attachments"] = repository::attachments(&*tx, mid).await?;
        if item["response"].is_object() {
            item["response"]["permissions"] = json!(
                repository::permissions(
                    &*tx,
                    &actor,
                    mid,
                    actor.roles.iter().any(|role| role == "approver")
                )
                .await?
            );
        }
    }
    let next = if more {
        items.first().map(|r| r["id"].clone())
    } else {
        None
    };
    tx.commit().await?;
    Ok(
        json!({"case_version":case["version"],"session_id":session,"items":items,"next_before":next,"settings":settings,"context_usage":context_usage,"config":integrations::config(),"can_send":actor.roles.iter().any(|r|r=="planner"),"runtime":if app.model_key.is_some(){json!({"state":"available"})}else{json!({"state":"offline","reason":"bridge_not_connected"})}}),
    )
}
