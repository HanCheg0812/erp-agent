use super::super::{
    repository,
    types::{ConversationInput, parse_case},
};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};

pub(crate) async fn create(app: &App, actor: &Principal, value: Value) -> Result<Value> {
    actor.require("planner")?;
    let input = parse_case(value)?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let record = repository::insert_case(&tx, actor, &input).await?;
    repository::audit(
        &*tx,
        actor,
        uuid(record["id"].as_str().unwrap())?,
        "case.created",
        json!({"version":1}),
    )
    .await?;
    tx.commit().await?;
    Ok(record)
}

pub(crate) async fn start_conversation(
    app: &App,
    actor: &Principal,
    value: Value,
) -> Result<(bool, Value)> {
    actor.require("planner")?;
    let input: ConversationInput = parse(value)?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let result = if let Some(record) =
        repository::insert_conversation(&tx, actor, input.request_id).await?
    {
        repository::audit(
            &*tx,
            actor,
            uuid(record["id"].as_str().unwrap())?,
            "conversation.created",
            json!({"version":1}),
        )
        .await?;
        (true, record)
    } else {
        (
            false,
            repository::conversation_by_request(&tx, actor, input.request_id).await?,
        )
    };
    tx.commit().await?;
    Ok(result)
}
