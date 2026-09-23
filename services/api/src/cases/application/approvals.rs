use super::super::{domain, repository};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) async fn approve(
    app: &App,
    actor: &Principal,
    id: Uuid,
    key: Option<&str>,
    value: Value,
) -> Result<(bool, Value)> {
    actor.require("approver")?;
    let input = domain::approval_input(value)?;
    let key = domain::approval_key(key)?;
    let hash = domain::approval_digest(id, &input);
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    repository::lock_approval_key(&tx, actor, key).await?;
    let current = repository::find(&*tx, actor, id, true).await?;
    if let Some(prior) = repository::approval_by_key(&tx, actor, key).await? {
        if prior["request_digest"] != hash {
            return Err(Error::conflict("idempotency_conflict"));
        }
        return Ok((false, json!({"approval":prior,"replayed":true})));
    }
    domain::can_approve(&current, &input)?;
    let proposal_digest = repository::proposal_digest(
        &tx,
        actor,
        id,
        input.proposal_id,
        input.expected_case_version,
    )
    .await?;
    if proposal_digest.as_deref() != Some(input.proposal_digest.as_str()) {
        return Err(Error::conflict("stale_proposal"));
    }
    let record = repository::insert_approval(&tx, actor, id, &input, key, &hash).await?;
    repository::mark_decision(&tx, id, &input.decision).await?;
    repository::audit(&*tx, actor, id, &format!("proposal.{}", input.decision), json!({"proposal_id":input.proposal_id,"digest":input.proposal_digest,"approval_id":record["id"]})).await?;
    let result = json!({"status":input.decision,"approval":record,"erp_executed":false});
    repository::complete_approval_requests(&tx, actor, id, &input, &result).await?;
    tx.commit().await?;
    Ok((true, json!({"approval":record,"replayed":false})))
}
