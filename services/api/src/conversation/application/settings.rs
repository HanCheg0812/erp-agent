use super::super::repository;
use crate::{agent_sessions, integrations::Settings};
use crate::{auth::Principal, cases, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn update_settings(
    app: App,
    actor: Principal,
    id: Uuid,
    value: Value,
) -> Result<Value> {
    actor.require("planner")?;
    let settings: Settings = parse(value)?;
    settings.validate()?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, &actor, id, true).await?;
    if repository::active_run(&*tx, &actor, id).await? {
        return Err(Error::conflict("agent_busy"));
    }
    let session = agent_sessions::bind(&tx, &actor, id).await?;
    let settings_value = json!(settings);
    repository::update_settings(&tx, &actor, id, &settings_value).await?;
    cases::audit(
        &*tx,
        &actor,
        id,
        "conversation.settings_updated",
        json!({"session_id":session,"settings":settings_value.clone()}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"settings":settings_value}))
}
