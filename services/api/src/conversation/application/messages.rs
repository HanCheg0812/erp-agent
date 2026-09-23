use super::super::{domain, repository};
use crate::{agent_sessions, integrations::Settings};
use crate::{auth::Principal, cases, platform::*};
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    client_message_id: Uuid,
    text: String,
    #[serde(default)]
    attachment_ids: Vec<Uuid>,
    #[serde(default)]
    settings: Settings,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Compact {
    request_id: Uuid,
}
pub(crate) async fn send(
    app: App,
    actor: Principal,
    id: Uuid,
    value: Value,
) -> Result<(bool, Value)> {
    let input: Input = parse(value)?;
    enqueue(
        &app,
        &actor,
        id,
        input.client_message_id,
        input.text.trim(),
        &input.attachment_ids,
        domain::Operation::Message,
        input.settings,
    )
    .await
}
pub(crate) async fn compact(
    app: App,
    actor: Principal,
    id: Uuid,
    value: Value,
) -> Result<(bool, Value)> {
    if app.model_key.is_none() {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "runtime_unavailable",
            "Context compression is unavailable",
        ));
    }
    let input: Compact = parse(value)?;
    enqueue(
        &app,
        &actor,
        id,
        input.request_id,
        "Compress conversation context",
        &[],
        domain::Operation::Compact,
        Settings::default(),
    )
    .await
}
async fn enqueue(
    app: &App,
    actor: &Principal,
    id: Uuid,
    request: Uuid,
    text_value: &str,
    attachment_ids: &[Uuid],
    operation: domain::Operation,
    settings: Settings,
) -> Result<(bool, Value)> {
    actor.require("planner")?;
    settings.validate()?;
    if attachment_ids.len() > 4
        || attachment_ids
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != attachment_ids.len()
    {
        return Err(Error::bad("Choose up to four distinct attachments"));
    }
    if operation == domain::Operation::Compact && !attachment_ids.is_empty()
        || text_value.is_empty() && attachment_ids.is_empty()
        || text_value.chars().count() > 4000
    {
        return Err(Error::bad("Invalid message content"));
    }
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, actor, id, true).await?;
    let mut settings = json!(settings);
    let prior = repository::existing_message(&*tx, actor, id, request).await?;
    if operation == domain::Operation::Compact {
        if let Some((message, _)) = &prior {
            settings = message["settings"].clone();
        } else if let Some(value) = repository::settings(&*tx, actor, id).await? {
            settings = value;
        }
    }
    if let Some((message, prior_operation)) = prior {
        let mut message = message;
        let mid = message["id"].as_str().unwrap().parse::<i64>().unwrap();
        message["attachments"] = repository::attachments(&*tx, mid).await?;
        domain::check_replay(
            &message,
            &prior_operation,
            text_value,
            &settings,
            attachment_ids,
            operation,
        )?;
        return Ok((false, json!({"message":message,"replayed":true})));
    }
    if repository::active_run(&*tx, actor, id).await? {
        return Err(Error::conflict("agent_busy"));
    }
    let session = agent_sessions::bind(&tx, actor, id).await?;
    let mut message =
        repository::insert_message(&tx, actor, id, request, text_value, &settings).await?;
    if operation == domain::Operation::Message {
        repository::update_settings(&tx, actor, id, &settings).await?;
    }
    let mid = message["id"].as_str().unwrap().parse::<i64>().unwrap();
    message["attachments"] = repository::attach(&tx, actor, id, mid, attachment_ids).await?;
    let attached = message["attachments"].as_array().unwrap();
    if attached
        .iter()
        .map(|item| item["bytes"].as_u64().unwrap_or(0))
        .sum::<u64>()
        > 5 * 1024 * 1024
    {
        return Err(Error::bad("Attachments exceed 5 MB total"));
    }
    if settings["model"] != "deepseek-flash"
        && attached.iter().any(|item| {
            item["content_type"]
                .as_str()
                .unwrap_or("")
                .starts_with("image/")
        })
    {
        return Err(Error::bad("Image input requires DeepSeek Flash"));
    }
    if operation == domain::Operation::Message {
        let title = if text_value.is_empty() {
            "Attachment conversation".to_owned()
        } else {
            domain::title(text_value)
        };
        repository::update_title(&tx, id, &title, mid).await?;
    }
    if app.model_key.is_some() {
        repository::enqueue_run(&tx, mid, actor, id, operation.as_str(), &settings).await?;
        message["response"] =
            json!({"state":"queued","text":"","steps":[],"tools":[],"compactions":[],"error":null});
    }
    message["operation"] = json!(operation.as_str());
    cases::audit(
        &*tx,
        actor,
        id,
        "conversation.message_saved",
        json!({"message_id":mid.to_string(),"session_id":session,"operation":operation.as_str()}),
    )
    .await?;
    tx.commit().await?;
    if app.model_key.is_some() {
        app.agent_wake.notify_one();
    }
    Ok((true, json!({"message":message,"replayed":false})))
}
