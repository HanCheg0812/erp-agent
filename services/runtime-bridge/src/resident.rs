//! Cube-facing resident JCode service.
//!
//! The process owns one Agent/session and reuses its provider, tool registry,
//! MCP connections, skill catalog and compaction state across turns. Cube
//! should run one instance per isolated session identity; concurrent turns for
//! that instance are rejected instead of interleaving a transcript.

use axum::{
    Router,
    body::{Body, Bytes, to_bytes},
    extract::State,
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    convert::Infallible,
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{Mutex, mpsc};
use tokio_stream::wrappers::ReceiverStream;

use crate::runtime::{self, ResidentRuntime};

const MAX_REQUEST_BYTES: usize = 12 * 1024 * 1024;
const MAX_CONFIG_BYTES: usize = 256 * 1024;

fn safe_failure_detail(error: &anyhow::Error) -> String {
    let mut detail = format!("{error:#}");
    for key in [
        "OPENAI_API_KEY",
        "JCODE_SESSION_STORE_TOKEN",
        "JCODE_MCP_SESSION_TOKEN",
        "AGENT_RUNTIME_AUTH_TOKEN",
        "ERP_API_TOKEN",
        "OPENVIKING_API_KEY",
    ] {
        if let Ok(value) = std::env::var(key) {
            if !value.is_empty() {
                detail = detail.replace(&value, "[redacted]");
            }
        }
    }
    detail.chars().take(512).collect()
}

#[derive(Clone, Default)]
struct RuntimeStatus {
    ready: bool,
    warmup: bool,
    auth_token: Option<String>,
}

#[derive(Clone)]
struct StateData {
    status: Arc<Mutex<RuntimeStatus>>,
    executable: PathBuf,
    gate: Arc<Mutex<()>>,
    runtime: Arc<Mutex<Option<ResidentRuntime>>>,
    agent_initialized: Arc<AtomicBool>,
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigureRequest {
    env: BTreeMap<String, String>,
}

pub async fn run() -> anyhow::Result<()> {
    let executable = std::env::var_os("AGENT_RUNTIME_BINARY")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_exe()?);
    let warmup = std::env::var("CUBE_TEMPLATE_WARMUP").ok().as_deref() == Some("1");
    let ready = bootstrap_ready(warmup, &executable);
    let auth_token = std::env::var("AGENT_RUNTIME_AUTH_TOKEN").ok();
    let state = StateData {
        status: Arc::new(Mutex::new(RuntimeStatus {
            ready,
            warmup,
            auth_token,
        })),
        executable,
        gate: Arc::new(Mutex::new(())),
        runtime: Arc::new(Mutex::new(None)),
        agent_initialized: Arc::new(AtomicBool::new(false)),
    };
    let bind = std::env::var("AGENT_RUNTIME_BIND").unwrap_or_else(|_| "0.0.0.0:8080".into());
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    let app = Router::new()
        .route("/health/live", get(live))
        .route("/health/ready", get(ready_probe))
        .route("/v1/configure", post(configure))
        .route("/v1/reconnect", post(reconnect))
        .route("/v1/turn", post(turn))
        .with_state(state);
    axum::serve(listener, app).await?;
    Ok(())
}

fn bootstrap_ready(warmup: bool, executable: &PathBuf) -> bool {
    if !executable.is_file() {
        return false;
    }
    // A template has no user/session secrets by design. A real instance must
    // have the provider, PostgreSQL session store, MCP bridge and auth token.
    if warmup {
        return true;
    }
    let mcp_binary = std::env::var_os("AGENT_MCP_BINARY").map(PathBuf::from);
    mcp_binary.is_some_and(|path| path.is_file())
        && [
            "OPENAI_API_KEY",
            "JCODE_OPENAI_MODEL",
            "JCODE_SESSION_STORE_ID",
            "JCODE_SESSION_STORE_URL",
            "JCODE_SESSION_STORE_TOKEN",
            "AGENT_MCP_BINARY",
            "AGENT_RUNTIME_AUTH_TOKEN",
        ]
        .iter()
        .all(|key| std::env::var(key).is_ok_and(|value| !value.trim().is_empty()))
}

async fn live() -> Response {
    json_response(StatusCode::OK, json!({"status":"live"}))
}

async fn ready_probe(State(state): State<StateData>) -> Response {
    let status = state.status.lock().await.clone();
    if !status.ready {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"status":"not_ready","stage":"configuration"}),
        );
    }
    let initialized = state.agent_initialized.load(Ordering::Acquire);
    json_response(
        StatusCode::OK,
        json!({
            "status":"ready",
            "mode":"resident",
            "warmup":status.warmup,
            "resident_agent":true,
            "agent_initialized":initialized,
            "concurrency":"one turn per instance"
        }),
    )
}

fn allowed_config_key(key: &str) -> bool {
    matches!(
        key,
        "CUBE_TEMPLATE_WARMUP"
            | "AGENT_RUNTIME_MODE"
            | "AGENT_RUNTIME_BIND"
            | "AGENT_RUNTIME_AUTH_TOKEN"
            | "AGENT_MCP_BINARY"
            | "AGENT_TURN_TIMEOUT_SECONDS"
            | "AGENT_COMPACTION_SOFT_RATIO"
            | "AGENT_COMPACTION_CRITICAL_RATIO"
            | "FORGE_TEMP_BASH_PROBE"
            | "OPENAI_API_KEY"
            | "JCODE_OPENAI_MODEL"
            | "JCODE_OPENAI_API_BASE"
            | "JCODE_OPENAI_TRANSPORT"
            | "JCODE_OPENAI_NATIVE_COMPACTION_MODE"
            | "JCODE_OPENAI_MAX_OUTPUT_TOKENS"
            | "JCODE_MAX_RETRIES"
            | "JCODE_MEMORY_ENABLED"
            | "JCODE_MEMORY_SIDECAR_ENABLED"
            | "JCODE_NO_TELEMETRY"
            | "DO_NOT_TRACK"
            | "JCODE_PRESERVE_REASONING_CONTEXT"
            | "JCODE_SESSION_STORE_URL"
            | "JCODE_SESSION_STORE_ID"
            | "JCODE_SESSION_STORE_TOKEN"
            | "JCODE_SESSION_STORE_ALLOW_HTTP_ORIGIN"
            | "JCODE_MCP_SESSION_ID"
            | "JCODE_MCP_SESSION_TOKEN"
            | "ERP_SHARED_DEMO_MODE"
            | "ERP_API_TOKEN"
            | "OPENVIKING_URL"
            | "OPENVIKING_API_KEY"
    ) || key.starts_with("ERP_CREDENTIAL_")
}

async fn configure(State(state): State<StateData>, headers: HeaderMap, body: Body) -> Response {
    let token = match headers
        .get("x-agent-runtime-token")
        .and_then(|value| value.to_str().ok())
        .filter(|value| value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit()))
    {
        Some(value) => value.to_owned(),
        None => return json_response(StatusCode::UNAUTHORIZED, json!({"error":"unauthorized"})),
    };
    let bytes = match to_bytes(body, MAX_CONFIG_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"configuration_too_large"}),
            );
        }
    };
    let mut request: ConfigureRequest = match serde_json::from_slice(&bytes) {
        Ok(request) => request,
        Err(_) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_configuration"}),
            );
        }
    };
    if request.env.keys().any(|key| !allowed_config_key(key)) {
        return json_response(
            StatusCode::BAD_REQUEST,
            json!({"error":"unsupported_configuration_key"}),
        );
    }
    if request.env.get("AGENT_RUNTIME_AUTH_TOKEN") != Some(&token) {
        return json_response(StatusCode::UNAUTHORIZED, json!({"error":"unauthorized"}));
    }
    request
        .env
        .insert("CUBE_TEMPLATE_WARMUP".to_owned(), "0".to_owned());
    request
        .env
        .insert("AGENT_RUNTIME_MODE".to_owned(), "resident".to_owned());

    let mut status = state.status.lock().await;
    if !status.warmup {
        return json_response(StatusCode::CONFLICT, json!({"error":"already_configured"}));
    }
    if state.runtime.lock().await.is_some() {
        return json_response(
            StatusCode::CONFLICT,
            json!({"error":"runtime_already_initialized"}),
        );
    }

    // The resident template has no envd sidecar, so Cube create-time envVars
    // cannot initialize it. Install only API-generated values here before
    // constructing JCode; the endpoint is one-shot and token-bound.
    unsafe {
        for (key, value) in &request.env {
            std::env::set_var(key, value);
        }
    }
    if !bootstrap_ready(false, &state.executable) {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"runtime_not_ready"}),
        );
    }
    status.ready = true;
    status.warmup = false;
    status.auth_token = Some(token);
    json_response(StatusCode::OK, json!({"status":"ready","warmup":false}))
}

async fn turn(State(state): State<StateData>, headers: HeaderMap, body: Body) -> Response {
    let status = state.status.lock().await.clone();
    if status.warmup {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"template_warmup"}),
        );
    }
    if !status.ready {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"runtime_not_ready"}),
        );
    }
    if let Some(expected) = status.auth_token.as_deref() {
        let actual = headers
            .get("x-agent-runtime-token")
            .and_then(|value| value.to_str().ok());
        if actual != Some(expected) {
            return json_response(StatusCode::UNAUTHORIZED, json!({"error":"unauthorized"}));
        }
    } else if !status.warmup {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"runtime_auth_not_configured"}),
        );
    }
    let permit = match state.gate.clone().try_lock_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return json_response(
                StatusCode::CONFLICT,
                json!({"error":"session_busy","retryable":true}),
            );
        }
    };
    let input = match to_bytes(body, MAX_REQUEST_BYTES).await {
        Ok(bytes) => bytes,
        Err(_) => {
            return json_response(
                StatusCode::PAYLOAD_TOO_LARGE,
                json!({"error":"request_too_large"}),
            );
        }
    };
    let request = match runtime::parse_request(&input) {
        Ok(request) => request,
        Err(error) => {
            return json_response(
                StatusCode::BAD_REQUEST,
                json!({"error":"invalid_request","detail":error.to_string()}),
            );
        }
    };
    let (output_tx, mut output_rx) = mpsc::unbounded_channel();
    let (body_tx, body_rx) = mpsc::channel::<Result<Bytes, Infallible>>(32);
    tokio::spawn(async move {
        let work = tokio::spawn(async move {
            let _permit = permit;
            if let Ok(delay) = std::env::var("CUBE_RESIDENT_TEST_DELAY_MS") {
                if let Ok(delay) = delay.parse::<u64>() {
                    tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
                }
            }
            let mut resident = state.runtime.lock().await;
            let result = async {
                if resident.is_some() && request.new_session {
                    anyhow::bail!("Resident instance is already bound to a JCode session");
                }
                let reused = resident.is_some();
                let build_started = std::time::Instant::now();
                if resident.is_none() {
                    *resident = Some(runtime::build(&request).await?);
                    state.agent_initialized.store(true, Ordering::Release);
                }
                let build_ms = build_started.elapsed().as_millis() as u64;
                let runtime = resident.as_mut().expect("resident initialized");
                if !runtime.accepts_case(&request) {
                    anyhow::bail!("Resident instance is bound to another case");
                }
                output_tx.send(runtime.catalog_event())?;
                output_tx.send(json!({
                    "type":"runtime_timing",
                    "reused":reused,
                    "build_ms":build_ms,
                    "model_catalog_ms":if reused { 0 } else { runtime.model_catalog_ms }
                }))?;
                runtime::turn(runtime, &request, output_tx.clone()).await
            }
            .await;
            if let Err(error) = result {
                let detail = safe_failure_detail(&error);
                eprintln!("Cube resident turn failed: {detail}");
                let _ = output_tx.send(json!({"type":"failed","code":"runtime","detail":detail}));
            }
        });
        let mut connected = true;
        while let Some(event) = output_rx.recv().await {
            if connected {
                let line = format!("{event}\n");
                connected = body_tx.send(Ok(Bytes::from(line))).await.is_ok();
            }
        }
        let _ = work.await;
    });
    ndjson_response(body_rx)
}

async fn reconnect(State(state): State<StateData>, headers: HeaderMap) -> Response {
    let status = state.status.lock().await.clone();
    if !status.ready || status.warmup {
        return json_response(
            StatusCode::SERVICE_UNAVAILABLE,
            json!({"error":"runtime_not_ready"}),
        );
    }
    let actual = headers
        .get("x-agent-runtime-token")
        .and_then(|value| value.to_str().ok());
    if actual != status.auth_token.as_deref() {
        return json_response(StatusCode::UNAUTHORIZED, json!({"error":"unauthorized"}));
    }
    let _permit = match state.gate.clone().try_lock_owned() {
        Ok(permit) => permit,
        Err(_) => {
            return json_response(
                StatusCode::CONFLICT,
                json!({"error":"session_busy","retryable":true}),
            );
        }
    };
    *state.runtime.lock().await = None;
    state.agent_initialized.store(false, Ordering::Release);
    json_response(StatusCode::OK, json!({"status":"reconnect_required"}))
}

fn json_response(status: StatusCode, value: Value) -> Response {
    let mut response = Response::new(Body::from(value.to_string()));
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json; charset=utf-8"),
    );
    response
}

fn ndjson_response(events: mpsc::Receiver<Result<Bytes, Infallible>>) -> Response {
    let mut response = Response::new(Body::from_stream(ReceiverStream::new(events)));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson"),
    );
    response
}
