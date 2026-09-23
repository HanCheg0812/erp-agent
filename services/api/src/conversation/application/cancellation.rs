use super::super::{domain, repository};
use crate::{auth::Principal, cases, platform::*};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn cancel(app: App, actor: Principal, id: Uuid, input: Value) -> Result<Value> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Cancel {
        message_id: String,
    }
    let input: Cancel = parse(input)?;
    let message: i64 = input
        .message_id
        .parse()
        .map_err(|_| Error::bad("Invalid message id"))?;
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, &actor, id, true).await?;
    let (mut state, cancel_requested) =
        repository::run_for_message(&*tx, &actor, id, message).await?;
    if let Some(next) = domain::cancellation_transition(&state, cancel_requested) {
        state = next.to_owned();
        repository::cancel_run(&tx, message, &state).await?;
        cases::audit(
            &*tx,
            &actor,
            id,
            "agent.cancel_requested",
            json!({"message_id":input.message_id}),
        )
        .await?;
    }
    tx.commit().await?;
    Ok(json!({"state":state}))
}
