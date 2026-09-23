use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Action {
    pub(crate) tool_call_id: String,
    pub(crate) tool: String,
    pub(crate) arguments: Value,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Memory {
    pub(crate) text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposalRequest {
    pub(crate) proposal_id: Uuid,
    pub(crate) proposal_digest: String,
    pub(crate) expected_case_version: i32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub(crate) decision: String,
    pub(crate) arguments_digest: String,
}
pub(crate) struct ActiveRun {
    pub(crate) tenant: Uuid,
    pub(crate) owner: Uuid,
    pub(crate) case: Uuid,
    pub(crate) message: i64,
    pub(crate) settings: Value,
}
