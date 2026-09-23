use super::super::{domain, repository::decisions as repository, types::Decision};
use crate::{auth::Principal, cases, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;
pub(crate) async fn decide(
    app: App,
    actor: Principal,
    (case, id): (Uuid, Uuid),
    input: Decision,
) -> Result<Value> {
    actor.require("planner")?;
    if !["allowed", "denied"].contains(&input.decision.as_str()) {
        return Err(Error::bad("Invalid decision"));
    }
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    cases::case(&*tx, &actor, case, true).await?;
    let mid = repository::message_id(&tx, &actor, case, id).await?;
    let (state, cancelled) = repository::lock_run(&tx, mid).await?;
    let permission = repository::lock_permission(&tx, id).await?;
    if domain::decision_replay(&permission, &input)? {
        return Ok(json!({"state":permission["state"],"replayed":true}));
    }
    let expired = repository::expired(&*tx, id).await?;
    domain::check_active_decision(&state, cancelled, expired)?;
    repository::record_decision(
        &tx,
        id,
        &input.decision,
        if input.decision == "allowed" {
            "用户批准"
        } else {
            "用户拒绝"
        },
    )
    .await?;
    repository::resume(&tx, mid).await?;
    cases::audit(&*tx,&actor,case,"agent.permission_decided",json!({"permission_id":id,"decision":input.decision,"arguments_digest":input.arguments_digest})).await?;
    tx.commit().await?;
    app.agent_wake.notify_one();
    Ok(json!({"state":input.decision,"replayed":false}))
}
