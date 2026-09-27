use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use deadpool_postgres::{Manager, Pool};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Instant,
};
use tokio_postgres::{GenericClient, NoTls};

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug)]
pub struct Error(pub StatusCode, pub &'static str, pub &'static str);
impl Error {
    pub fn bad(message: &'static str) -> Self {
        Self(StatusCode::BAD_REQUEST, "invalid_request", message)
    }
    pub fn conflict(code: &'static str) -> Self {
        Self(
            StatusCode::CONFLICT,
            code,
            "State changed; reload and retry",
        )
    }
    pub fn forbidden() -> Self {
        Self(StatusCode::FORBIDDEN, "forbidden", "Not permitted")
    }
    pub fn missing() -> Self {
        Self(StatusCode::NOT_FOUND, "not_found", "Record not found")
    }
    pub fn unauthorized() -> Self {
        Self(
            StatusCode::UNAUTHORIZED,
            "unauthorized",
            "Invalid credentials or expired session",
        )
    }
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.1, self.2)
    }
}
impl std::error::Error for Error {}

fn agent_guidance(code: &str) -> Option<Value> {
    let (category, retryable, safe_next_step, instruction) = match code {
        "platform_forbidden" => (
            "authorization",
            false,
            "ask_platform_admin",
            "Platform policy denied this tool or scope. Do not change arguments to bypass the policy or infer hidden data. Ask a platform administrator to review the task scope or grant.",
        ),
        "erp_forbidden" => (
            "authorization",
            false,
            "ask_erp_admin",
            "ERPNext denied the bound user. Do not retry with altered arguments or infer hidden records. Ask an ERPNext administrator to grant the required Role/User Permission or bind an authorized ERPNext user.",
        ),
        "erp_scope_missing" => (
            "authorization",
            false,
            "ask_user_or_platform_admin",
            "The current task scope does not include this customer or resource. Do not broaden the scope yourself; ask the user or a platform administrator to update the task scope.",
        ),
        "erp_identity_unbound" => (
            "configuration",
            false,
            "ask_platform_admin",
            "No ERPNext identity is bound to this platform user. Do not ask the user for an API token; ask a platform administrator to create the binding.",
        ),
        "erp_identity_inactive" => (
            "configuration",
            false,
            "ask_platform_admin",
            "The ERPNext identity binding is inactive. Ask a platform administrator to reactivate or replace the binding before retrying.",
        ),
        "erp_credentials_unavailable" => (
            "configuration",
            false,
            "ask_platform_admin",
            "The credential for the bound ERPNext identity is unavailable. Do not ask the user for a secret; ask a platform administrator to repair or rotate the server-side credential.",
        ),
        "erp_shared_identity_disabled" => (
            "configuration",
            false,
            "ask_platform_admin",
            "The shared ERPNext demo identity is disabled. Ask a platform administrator to configure a bound-user credential; do not retry with a different identity.",
        ),
        _ => return None,
    };
    Some(json!({
        "category": category,
        "retryable": retryable,
        "safe_next_step": safe_next_step,
        "instruction": instruction,
    }))
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let mut body = json!({"error":self.1,"message":self.2});
        if let Some(guidance) = agent_guidance(self.1) {
            body["agent_guidance"] = guidance;
        }
        (self.0, Json(body)).into_response()
    }
}
impl From<tokio_postgres::Error> for Error {
    fn from(e: tokio_postgres::Error) -> Self {
        eprintln!("Database failure: code={:?} message={e}", e.code());
        Self(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "Database operation failed",
        )
    }
}
impl From<deadpool_postgres::PoolError> for Error {
    fn from(_: deadpool_postgres::PoolError) -> Self {
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            "database_unavailable",
            "Database unavailable",
        )
    }
}
impl From<anyhow::Error> for Error {
    fn from(_: anyhow::Error) -> Self {
        Self(
            StatusCode::SERVICE_UNAVAILABLE,
            "service_unavailable",
            "Service operation failed",
        )
    }
}

#[derive(Clone)]
pub struct App {
    pub db: Pool,
    pub agent_wake: Arc<tokio::sync::Notify>,
    pub host: String,
    pub port: u16,
    pub model_key: Option<String>,
    pub http: reqwest::Client,
    pub passwords: Arc<tokio::sync::Semaphore>,
    pub attempts: Arc<Mutex<HashMap<String, (Instant, u32)>>>,
    pub bucket: Option<Arc<s3::Bucket>>,
}
impl App {
    pub async fn connect(pool_max_size: usize) -> anyhow::Result<Self> {
        let mut config: tokio_postgres::Config = std::env::var("DATABASE_URL")?.parse()?;
        config.connect_timeout(std::time::Duration::from_secs(5));
        let db = Pool::builder(Manager::new(config, NoTls))
            .max_size(pool_max_size)
            .runtime(deadpool_postgres::Runtime::Tokio1)
            .wait_timeout(Some(std::time::Duration::from_secs(5)))
            .create_timeout(Some(std::time::Duration::from_secs(5)))
            .build()?;
        let model_key = std::env::var("DEEPSEEK_API_KEY")
            .ok()
            .filter(|v| !v.is_empty());
        let bucket = crate::objects::bucket_from_env()?;
        Ok(Self {
            db,
            agent_wake: Arc::new(tokio::sync::Notify::new()),
            host: env("API_HOST", "127.0.0.1"),
            port: env("API_PORT", "3100").parse()?,
            model_key,
            http: reqwest::Client::builder().build()?,
            passwords: Arc::new(tokio::sync::Semaphore::new(4)),
            attempts: Default::default(),
            bucket,
        })
    }
}
pub fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.into())
}
pub fn digest(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}
pub fn json_digest(value: &Value) -> String {
    digest(serde_json::to_vec(value).expect("serializable JSON"))
}
pub async fn object<C: GenericClient + Sync>(
    db: &C,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Value> {
    db.query_opt(sql, params)
        .await?
        .map(|r| r.get(0))
        .ok_or_else(Error::missing)
}
pub async fn array<C: GenericClient + Sync>(
    db: &C,
    sql: &str,
    params: &[&(dyn tokio_postgres::types::ToSql + Sync)],
) -> Result<Vec<Value>> {
    Ok(db
        .query(sql, params)
        .await?
        .into_iter()
        .map(|r| r.get(0))
        .collect())
}
pub fn parse<T: serde::de::DeserializeOwned>(v: Value) -> Result<T> {
    serde_json::from_value(v).map_err(|_| Error::bad("Invalid request fields"))
}
pub fn text(value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.chars().count() > max {
        Err(Error::bad("Invalid text length"))
    } else {
        Ok(())
    }
}
pub fn uuid(value: &str) -> Result<uuid::Uuid> {
    uuid::Uuid::parse_str(value).map_err(|_| Error::bad("Invalid identifier"))
}
pub fn session_id(value: &str) -> Result<()> {
    if value.len() != 40
        || !value.starts_with("session_")
        || !value[8..]
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        Err(Error::bad("Invalid session identifier"))
    } else {
        Ok(())
    }
}

const MIGRATIONS: &[(&str, &str)] = &[
    (
        "20260921171235_create_auth_schema",
        include_str!("../../migrations/20260921171235_create_auth_schema.sql"),
    ),
    (
        "20260921171709_create_cases_and_proposals",
        include_str!("../../migrations/20260921171709_create_cases_and_proposals.sql"),
    ),
    (
        "20260921171710_create_approvals",
        include_str!("../../migrations/20260921171710_create_approvals.sql"),
    ),
    (
        "20260921171711_create_audit_events",
        include_str!("../../migrations/20260921171711_create_audit_events.sql"),
    ),
    (
        "20260921183302_create_agent_sessions",
        include_str!("../../migrations/20260921183302_create_agent_sessions.sql"),
    ),
    (
        "20260921232215_create_conversations",
        include_str!("../../migrations/20260921232215_create_conversations.sql"),
    ),
    (
        "20260921235008_create_case_sandboxes",
        include_str!("../../migrations/20260921235008_create_case_sandboxes.sql"),
    ),
    (
        "20260922003753_create_agent_runs",
        include_str!("../../migrations/20260922003753_create_agent_runs.sql"),
    ),
    (
        "20260922010102_add_agent_steps",
        include_str!("../../migrations/20260922010102_add_agent_steps.sql"),
    ),
    (
        "20260922020919_create_skill_catalogs",
        include_str!("../../migrations/20260922020919_create_skill_catalogs.sql"),
    ),
    (
        "20260922094751_add_compaction_events",
        include_str!("../../migrations/20260922094751_add_compaction_events.sql"),
    ),
    (
        "20260922100336_create_personal_memory",
        include_str!("../../migrations/20260922100336_create_personal_memory.sql"),
    ),
    (
        "20260922105658_reissue_runtime_leases",
        include_str!("../../migrations/20260922105658_reissue_runtime_leases.sql"),
    ),
    (
        "20260922130852_add_conversation_entries",
        include_str!("../../migrations/20260922130852_add_conversation_entries.sql"),
    ),
    (
        "20260922135145_add_agent_tool_settings",
        include_str!("../../migrations/20260922135145_add_agent_tool_settings.sql"),
    ),
    (
        "20260922154623_add_tool_permissions",
        include_str!("../../migrations/20260922154623_add_tool_permissions.sql"),
    ),
    (
        "20260922164200_add_stream_checkpoint",
        include_str!("../../migrations/20260922164200_add_stream_checkpoint.sql"),
    ),
    (
        "20260922172345_add_business_tool_approvals",
        include_str!("../../migrations/20260922172345_add_business_tool_approvals.sql"),
    ),
    (
        "20260924150000_release_cube_provider_ids",
        include_str!("../../migrations/20260924150000_release_cube_provider_ids.sql"),
    ),
    (
        "20260923043557_add_context_usage",
        include_str!("../../migrations/20260923043557_add_context_usage.sql"),
    ),
    (
        "20260923120000_message_attachments",
        include_str!("../../migrations/20260923120000_message_attachments.sql"),
    ),
    (
        "20260923180000_add_agent_first_token_time",
        include_str!("../../migrations/20260923180000_add_agent_first_token_time.sql"),
    ),
    (
        "20260923190000_agent_worker_wakeup",
        include_str!("../../migrations/20260923190000_agent_worker_wakeup.sql"),
    ),
    (
        "20260924100000_add_integration_tool_policies",
        include_str!("../../migrations/20260924100000_add_integration_tool_policies.sql"),
    ),
    (
        "20260924110000_add_erp_identity_bindings",
        include_str!("../../migrations/20260924110000_add_erp_identity_bindings.sql"),
    ),
    (
        "20260924120000_create_mcp_sessions",
        include_str!("../../migrations/20260924120000_create_mcp_sessions.sql"),
    ),
    (
        "20260924123000_add_cube_runtime_session",
        include_str!("../../migrations/20260924123000_add_cube_runtime_session.sql"),
    ),
    (
        "20260924130000_version_personal_memory",
        include_str!("../../migrations/20260924130000_version_personal_memory.sql"),
    ),
    (
        "20260924133000_add_memory_audit_events",
        include_str!("../../migrations/20260924133000_add_memory_audit_events.sql"),
    ),
    (
        "20260924160000_create_knowledge_embedding_jobs",
        include_str!("../../migrations/20260924160000_create_knowledge_embedding_jobs.sql"),
    ),
    (
        "20260925100000_add_agent_run_timings",
        include_str!("../../migrations/20260925100000_add_agent_run_timings.sql"),
    ),
    (
        "20260926100000_runtime_management",
        include_str!("../../migrations/20260926100000_runtime_management.sql"),
    ),
    (
        "20260926110000_cube_quantity_tool",
        include_str!("../../migrations/20260926110000_cube_quantity_tool.sql"),
    ),
    (
        "20260926120000_cube_browser_tool",
        include_str!("../../migrations/20260926120000_cube_browser_tool.sql"),
    ),
    (
        "20260926140000_cube_template_switch",
        include_str!("../../migrations/20260926140000_cube_template_switch.sql"),
    ),
];
const LEGACY_MIGRATION_IDS: &[(&str, &str)] = &[
    ("001_auth", "20260921171235_create_auth_schema"),
    ("002_cases", "20260921171709_create_cases_and_proposals"),
    ("003_approvals", "20260921171710_create_approvals"),
    ("004_audit", "20260921171711_create_audit_events"),
    ("005_agent_sessions", "20260921183302_create_agent_sessions"),
    ("006_conversation", "20260921232215_create_conversations"),
    ("007_sandboxes", "20260921235008_create_case_sandboxes"),
    ("008_agent_runs", "20260922003753_create_agent_runs"),
    ("009_agent_steps", "20260922010102_add_agent_steps"),
    ("010_skills", "20260922020919_create_skill_catalogs"),
    ("011_compactions", "20260922094751_add_compaction_events"),
    ("012_personal", "20260922100336_create_personal_memory"),
    (
        "013_runtime_cutover",
        "20260922105658_reissue_runtime_leases",
    ),
    (
        "014_conversation_entry",
        "20260922130852_add_conversation_entries",
    ),
    ("015_agent_tools", "20260922135145_add_agent_tool_settings"),
    (
        "016_tool_permissions",
        "20260922154623_add_tool_permissions",
    ),
    (
        "017_stream_checkpoint",
        "20260922164200_add_stream_checkpoint",
    ),
    (
        "018_business_tool_approvals",
        "20260922172345_add_business_tool_approvals",
    ),
    (
        "019_release_cube_provider_ids",
        "20260924150000_release_cube_provider_ids",
    ),
];
pub async fn migrate(app: &App) -> Result<()> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    tx.query_one("SELECT pg_advisory_xact_lock(82720260921)", &[])
        .await?;
    tx.batch_execute("CREATE TABLE IF NOT EXISTS schema_migrations(name text PRIMARY KEY,checksum text NOT NULL,applied_at timestamptz NOT NULL DEFAULT now())").await?;
    for (legacy_name, current_name) in LEGACY_MIGRATION_IDS {
        let Some(row) = tx
            .query_opt(
                "SELECT checksum FROM schema_migrations WHERE name=$1",
                &[legacy_name],
            )
            .await?
        else {
            continue;
        };
        let sql = MIGRATIONS
            .iter()
            .find(|(name, _)| name == current_name)
            .map(|(_, sql)| *sql)
            .ok_or_else(|| anyhow::anyhow!("migration registry mismatch"))?;
        if row.get::<_, String>(0) != digest(sql) {
            return Err(Error::conflict("migration_changed"));
        }
        tx.execute(
            "UPDATE schema_migrations SET name=$1 WHERE name=$2",
            &[current_name, legacy_name],
        )
        .await?;
    }
    for (name, sql) in MIGRATIONS {
        let hash = digest(sql);
        if let Some(row) = tx
            .query_opt(
                "SELECT checksum FROM schema_migrations WHERE name=$1",
                &[name],
            )
            .await?
        {
            if row.get::<_, String>(0) != hash {
                return Err(Error::conflict("migration_changed"));
            }
            continue;
        }
        tx.batch_execute(sql).await?;
        tx.execute(
            "INSERT INTO schema_migrations(name,checksum) VALUES($1,$2)",
            &[name, &hash],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}
pub async fn ready(app: &App) -> Result<()> {
    let db = app.db.get().await?;
    for (name, sql) in MIGRATIONS {
        let row = db
            .query_opt(
                "SELECT checksum FROM schema_migrations WHERE name=$1",
                &[name],
            )
            .await?;
        if row.is_none_or(|r| r.get::<_, String>(0) != digest(sql)) {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "migrations_pending",
                "Database migration required",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[test]
    fn every_sql_migration_is_registered() {
        let directory = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
        let mut files: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .filter(|name| name.ends_with(".sql"))
            .map(|name| name.trim_end_matches(".sql").to_owned())
            .collect();
        files.sort();
        let mut registered: Vec<_> = MIGRATIONS
            .iter()
            .map(|(name, _)| name.to_string())
            .collect();
        registered.sort();
        assert_eq!(files, registered);
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Input {
        value: i32,
    }

    #[test]
    fn hashing_parsing_text_uuid_and_session_guards_are_stable() {
        assert_eq!(
            digest("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
        assert_eq!(json_digest(&json!({"a":1})), digest(br#"{"a":1}"#));
        assert_eq!(parse::<Input>(json!({"value":7})).unwrap().value, 7);
        assert!(parse::<Input>(json!({"value":7,"extra":true})).is_err());
        assert!(text("value", 5).is_ok());
        assert!(text(" value ", 5).is_err());
        assert!(text("   ", 5).is_err());
        assert!(text("中文", 2).is_ok());
        assert!(text("中文件", 2).is_err());
        assert!(uuid("00000000-0000-0000-0000-000000000000").is_ok());
        assert!(uuid("not-a-uuid").is_err());
        assert!(session_id("session_0123456789abcdef0123456789abcdef").is_ok());
        for value in [
            "session_0123456789ABCDEF0123456789abcdef",
            "session_0123456789abcdef0123456789abcdeg",
            "0123456789abcdef0123456789abcdef",
        ] {
            assert!(session_id(value).is_err(), "{value}");
        }
    }

    #[test]
    fn agent_guidance_is_explicit_for_permission_errors() {
        let guidance = agent_guidance("erp_forbidden").unwrap();
        assert_eq!(guidance["category"], "authorization");
        assert_eq!(guidance["retryable"], false);
        assert_eq!(guidance["safe_next_step"], "ask_erp_admin");
        assert!(
            guidance["instruction"]
                .as_str()
                .unwrap()
                .contains("Do not retry with altered arguments")
        );
        assert!(agent_guidance("unknown_error").is_none());
    }

    #[test]
    fn public_error_constructors_keep_status_and_codes() {
        for (error, status, code) in [
            (
                Error::bad("bad"),
                StatusCode::BAD_REQUEST,
                "invalid_request",
            ),
            (Error::conflict("stale"), StatusCode::CONFLICT, "stale"),
            (Error::forbidden(), StatusCode::FORBIDDEN, "forbidden"),
            (Error::missing(), StatusCode::NOT_FOUND, "not_found"),
            (
                Error::unauthorized(),
                StatusCode::UNAUTHORIZED,
                "unauthorized",
            ),
        ] {
            assert_eq!(error.0, status);
            assert_eq!(error.1, code);
        }
    }
}
