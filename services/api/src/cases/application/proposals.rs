use super::super::{domain, repository};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
use tokio_postgres::Transaction;
use uuid::Uuid;

pub(crate) async fn propose(app: &App, actor: &Principal, id: Uuid, value: Value) -> Result<Value> {
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let proposal = propose_tx(&tx, actor, id, value).await?;
    tx.commit().await?;
    Ok(proposal)
}

pub(crate) async fn propose_tx(
    tx: &Transaction<'_>,
    actor: &Principal,
    id: Uuid,
    value: Value,
) -> Result<Value> {
    actor.require("planner")?;
    let input = domain::proposal_input(value)?;
    let current = repository::find(tx, actor, id, true).await?;
    domain::can_propose(&current, input.expected_case_version)?;
    for evidence in &input.evidence {
        if !repository::evidence_is_ready(tx, actor, id, evidence.document_id).await? {
            return Err(Error::bad(
                "Evidence must reference a document in this case",
            ));
        }
    }
    let version = domain::next_version(input.expected_case_version)?;
    let hash = json_digest(&json!({"caseId":id,"version":version,"proposal":input}));
    let proposal = repository::insert_proposal(tx, actor, id, &input, version, &hash).await?;
    repository::mark_awaiting_approval(tx, id, version).await?;
    repository::audit(
        tx,
        actor,
        id,
        "proposal.created",
        json!({"proposal_id":proposal["id"],"digest":hash,"version":version}),
    )
    .await?;
    repository::expire_proposal_requests(tx, id).await?;
    Ok(proposal)
}
