use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParserCapabilities {
    pub backend: String,
    pub model_refs: Vec<String>,
    pub mime_types: Vec<String>,
    pub regions: bool,
    pub tables: bool,
    pub coordinates: bool,
    pub asynchronous: bool,
    pub cancellation: bool,
    pub remote_idempotency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRef {
    pub artifact_id: String,
    pub sha256: String,
    pub bytes: u64,
    pub mime_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseRequest {
    pub tenant_id: String,
    pub revision_id: String,
    pub run_id: String,
    pub step_id: String,
    pub input: ArtifactRef,
    pub profile_hash: String,
    pub model_ref: String,
    pub options: Value,
    pub idempotency_key: String,
    pub deadline_unix_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RemoteJob {
    pub backend: String,
    pub remote_id: String,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawArtifactManifest {
    pub engine_version: String,
    pub model_version: String,
    pub artifacts: Vec<ArtifactRef>,
    pub covered_pages: Vec<u32>,
    pub missing_pages: Vec<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "result",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Submission {
    Ready(RawArtifactManifest),
    Pending(RemoteJob),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "state",
    content = "result",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum JobProgress {
    Pending,
    Ready(RawArtifactManifest),
    Failed(ParseError),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelOutcome {
    Cancelled,
    AlreadyFinished,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    UnsupportedInput,
    Configuration,
    Authentication,
    RateLimited,
    Unavailable,
    Permanent,
    Partial,
    UnknownState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ParseError {
    pub kind: ErrorKind,
    pub message: String,
    pub retry_after_seconds: Option<u32>,
}
impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}
impl std::error::Error for ParseError {}

#[async_trait]
pub trait ParseBackend: Send + Sync {
    fn capabilities(&self) -> &ParserCapabilities;
    // Each adapter owns its option schema; profile inheritance never bypasses it.
    fn validate_options(&self, options: &Value) -> Result<(), ParseError>;
    async fn submit(&self, request: &ParseRequest) -> Result<Submission, ParseError>;
    async fn poll(&self, handle: &RemoteJob) -> Result<JobProgress, ParseError>;
    async fn cancel(&self, handle: &RemoteJob) -> Result<CancelOutcome, ParseError>;
}
