use crate::{agent_sessions, auth::Principal, cases, mcp_sessions, objects, platform::*};
use anyhow::{Context, ensure};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    future::poll_fn,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_postgres::{AsyncMessage, NoTls};

#[derive(Debug)]
enum TurnFailure {
    Timeout,
    Interrupted,
    Runtime,
}
impl std::fmt::Display for TurnFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Runtime {:?}", self)
    }
}
impl std::error::Error for TurnFailure {}

#[derive(Default)]
struct CubeLines {
    pending: Vec<u8>,
    total: usize,
}

struct RunTiming {
    started: Instant,
    backend: &'static str,
    offsets: serde_json::Map<String, Value>,
    running_tools: HashMap<String, (String, Instant)>,
    tools: Vec<Value>,
    runtime: Option<Value>,
    lifecycle: Option<Value>,
}

impl RunTiming {
    fn new(backend: &'static str) -> Self {
        Self {
            started: Instant::now(),
            backend,
            offsets: serde_json::Map::new(),
            running_tools: HashMap::new(),
            tools: Vec::new(),
            runtime: None,
            lifecycle: None,
        }
    }

    fn mark(&mut self, name: &str) {
        self.offsets
            .entry(name.to_owned())
            .or_insert_with(|| json!(self.started.elapsed().as_millis() as u64));
    }

    fn event(&mut self, event: &Value) {
        match event["type"].as_str() {
            Some("skill_catalog") => self.mark("catalog"),
            Some("runtime_timing") => {
                self.runtime = Some(json!({
                    "reused": event["reused"],
                    "build_ms": event["build_ms"],
                    "model_catalog_ms": event["model_catalog_ms"]
                }));
            }
            Some("reasoning_delta") if event["text"].as_str().is_some_and(|s| !s.is_empty()) => {
                self.mark("first_reasoning");
            }
            Some("text_delta" | "text_replace")
                if event["text"].as_str().is_some_and(|s| !s.is_empty()) =>
            {
                self.mark("first_text");
            }
            Some("tool_start") => {
                self.mark("first_tool_start");
                if let (Some(id), Some(name)) = (event["id"].as_str(), event["name"].as_str()) {
                    self.running_tools
                        .insert(id.to_owned(), (name.to_owned(), Instant::now()));
                }
            }
            Some("tool_done") => {
                self.mark("first_tool_done");
                if let Some((name, started)) = event["id"]
                    .as_str()
                    .and_then(|id| self.running_tools.remove(id))
                {
                    self.tools.push(json!({
                        "name": name,
                        "duration_ms": started.elapsed().as_millis() as u64,
                        "failed": event["failed"] == true
                    }));
                }
            }
            _ => {}
        }
    }

    fn snapshot(&self) -> Value {
        json!({
            "version": 1,
            "backend": self.backend,
            "offsets_ms": self.offsets,
            "tools": self.tools,
            "runtime": self.runtime,
            "lifecycle": self.lifecycle,
            "elapsed_ms": self.started.elapsed().as_millis() as u64
        })
    }
}
impl CubeLines {
    fn push(&mut self, chunk: &[u8]) -> anyhow::Result<Vec<Value>> {
        self.total += chunk.len();
        ensure!(
            self.total <= 2 * 1024 * 1024,
            "Cube runtime response too large"
        );
        self.pending.extend_from_slice(chunk);
        let mut events = Vec::new();
        let mut consumed = 0;
        for (index, byte) in self.pending.iter().enumerate() {
            if *byte == b'\n' {
                let line = &self.pending[consumed..index];
                ensure!(line.len() <= 128 * 1024, "Cube runtime event too large");
                if !line.is_empty() {
                    events
                        .push(serde_json::from_slice(line).context("Invalid Cube runtime event")?);
                }
                consumed = index + 1;
            }
        }
        self.pending.drain(..consumed);
        ensure!(
            self.pending.len() <= 128 * 1024,
            "Cube runtime event too large"
        );
        Ok(events)
    }

    fn finish(&self) -> anyhow::Result<()> {
        ensure!(self.pending.is_empty(), "Incomplete Cube runtime event");
        Ok(())
    }
}

#[derive(Default)]
struct Reply {
    text: String,
    tools: Vec<Value>,
    steps: Vec<Value>,
    compactions: Vec<Value>,
    context_usage: Option<Value>,
    boundary: bool,
    suspended: bool,
}

fn temporary_bash_probe_enabled() -> bool {
    matches!(std::env::var("AGENT_TEMP_BASH_PROBE").as_deref(), Ok("1"))
}

fn tool_is_published(name: &str, bash_enabled: bool) -> bool {
    crate::integrations::TOOLS.contains(&name) || (bash_enabled && name == "bash")
}

impl Reply {
    fn close(&mut self, state: &str) {
        for step in &mut self.steps {
            if step["kind"] == "reasoning" && step["state"] == "running" {
                step["state"] = json!(state)
            }
        }
    }
    fn event(&mut self, event: &Value) -> anyhow::Result<()> {
        let kind = event["type"].as_str().context("Event type missing")?;
        if ["reasoning_done", "text_delta", "tool_start", "completed"].contains(&kind) {
            self.close("completed")
        }
        match kind {
            "reasoning_delta" => {
                let text = event["text"].as_str().context("Reasoning missing")?;
                ensure!(text.len() <= 65536, "Reasoning delta limit");
                if self
                    .steps
                    .last()
                    .is_none_or(|s| s["kind"] != "reasoning" || s["state"] != "running")
                {
                    self.steps.push(json!({"id":format!("reasoning-{}",self.steps.len()),"kind":"reasoning","text":"","state":"running"}));
                }
                let step = self.steps.last_mut().unwrap();
                let mut value = step["text"].as_str().unwrap().to_owned();
                value.push_str(text);
                step["text"] = json!(value);
            }
            "text_delta" => {
                let text = event["text"].as_str().context("Text missing")?;
                if self.boundary && !self.text.is_empty() {
                    self.text.push_str("\n\n")
                }
                self.boundary = false;
                self.text.push_str(text);
            }
            "text_replace" => self.text = event["text"].as_str().context("Text missing")?.into(),
            "text_done" => self.boundary = true,
            "tool_start" | "tool_done" => {
                let id = event["id"].as_str().context("Tool id missing")?;
                let name = event["name"].as_str().context("Tool name missing")?;
                ensure!(
                    id.len() <= 256 && tool_is_published(name, temporary_bash_probe_enabled()),
                    "Unpublished tool"
                );
                if kind == "tool_start" && !self.tools.iter().any(|t| t["id"] == id) {
                    self.tools
                        .push(json!({"id":id,"name":name,"state":"running"}));
                    self.steps.push(json!({"id":id,"kind":"tool"}));
                }
                if kind == "tool_done" {
                    let tool = self
                        .tools
                        .iter_mut()
                        .find(|t| t["id"] == id && t["name"] == name)
                        .context("Unknown tool completion")?;
                    tool["state"] = json!(if event["failed"]
                        .as_bool()
                        .context("Tool status missing")?
                    {
                        "failed"
                    } else {
                        "completed"
                    });
                }
            }
            "compaction" => {
                ensure!(
                    event["trigger"].as_str().is_some_and(|s| s.len() <= 64),
                    "Invalid compaction trigger"
                );
                let mut compact = json!({"trigger":event["trigger"]});
                for key in [
                    "pre_tokens",
                    "post_tokens",
                    "tokens_saved",
                    "duration_ms",
                    "messages_dropped",
                    "messages_compacted",
                    "summary_chars",
                    "active_messages",
                ] {
                    ensure!(
                        event
                            .get(key)
                            .is_some_and(|v| v.is_null() || v.as_u64().is_some()),
                        "Invalid compaction metric"
                    );
                    compact[key] = event[key].clone();
                }
                self.compactions.push(compact);
            }
            "context_usage" => {
                let model = event["model"].as_str().context("Context model missing")?;
                let used = event["used_tokens"]
                    .as_u64()
                    .context("Context token usage missing")?;
                let window = event["window_tokens"]
                    .as_u64()
                    .context("Context window missing")?;
                let soft = event["soft_ratio"]
                    .as_f64()
                    .context("Context soft ratio missing")?;
                let critical = event["critical_ratio"]
                    .as_f64()
                    .context("Context critical ratio missing")?;
                ensure!(
                    crate::integrations::model_context_window(model).is_some()
                        && window > 0
                        && used <= 100_000_000
                        && 0.0 < soft
                        && soft < critical
                        && critical < 1.0
                        && event["compacting"].is_boolean()
                        && event["estimated"].is_boolean(),
                    "Invalid context usage"
                );
                self.context_usage = Some(json!({
                    "model":model,
                    "used_tokens":used,
                    "window_tokens":window,
                    "soft_ratio":soft,
                    "critical_ratio":critical,
                    "compacting":event["compacting"],
                    "estimated":event["estimated"]
                }));
            }
            "reasoning_done" | "completed" | "compaction_started" => {}
            _ => anyhow::bail!("Unknown runtime event"),
        }
        ensure!(
            self.text.len() <= 65536
                && self.tools.len() <= 32
                && self.steps.len() <= 128
                && self.compactions.len() <= 16
                && self
                    .steps
                    .iter()
                    .map(|s| s["text"].as_str().unwrap_or("").len())
                    .sum::<usize>()
                    <= 131072,
            "Turn output limit"
        );
        Ok(())
    }
    async fn save(&self, db: &tokio_postgres::Client, id: i64) -> anyhow::Result<()> {
        db.execute("UPDATE agent_runs SET text=$2,tools=$3,steps=$4,compactions=$5,context_usage=$6,updated_at=now() WHERE message_id=$1",&[&id,&self.text,&json!(self.tools),&json!(self.steps),&json!(self.compactions),&self.context_usage]).await?;
        Ok(())
    }
}
fn catalog(snapshot: &str, hash: &str) -> anyhow::Result<Value> {
    ensure!(
        snapshot.len() <= 65536 && digest(snapshot) == hash,
        "Catalog digest mismatch"
    );
    let value: Value = serde_json::from_str(snapshot)?;
    ensure!(
        value["schema_version"] == 1
            && value["id"]
                .as_str()
                .is_some_and(|s| !s.is_empty() && s.len() <= 100),
        "Invalid catalog"
    );
    let skills = value["skills"]
        .as_array()
        .context("Missing catalog skills")?;
    ensure!(!skills.is_empty() && skills.len() <= 32, "Catalog limit");
    let mut ids = HashSet::new();
    for skill in skills {
        let id = skill["id"].as_str().context("Skill id missing")?;
        ensure!(
            !id.is_empty()
                && id.len() <= 63
                && id
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                && ids.insert(id),
            "Invalid skill id"
        );
        let source = skill["source"].as_str().context("Skill source missing")?;
        ensure!(
            source.len() <= 16384
                && digest(source) == skill["sha256"].as_str().unwrap_or("")
                && skill["version"]
                    .as_str()
                    .is_some_and(|s| !s.is_empty() && s.len() <= 64),
            "Invalid skill source"
        );
    }
    Ok(value)
}
async fn turn(
    app: &App,
    db: &tokio_postgres::Client,
    row: &Value,
    reply: &mut Reply,
    stop: &tokio_util::sync::CancellationToken,
    timing: &mut RunTiming,
) -> anyhow::Result<()> {
    if crate::sandboxes::bridge::enabled() {
        return turn_cube(app, db, row, reply, stop, timing).await;
    }
    let actor = Principal {
        tenant_id: uuid(row["tenant_id"].as_str().unwrap())?,
        user_id: uuid(row["actor_id"].as_str().unwrap())?,
        roles: serde_json::from_value(row["roles"].clone())?,
        email: String::new(),
    };
    let id = uuid(row["case_id"].as_str().unwrap())?;
    let mid = row["message_id"].as_i64().unwrap();
    let mut record = cases::case(db, &actor, id, false).await?;
    record["proposals"]=json!(array(db,"SELECT to_jsonb(p) FROM proposals p WHERE tenant_id=$1 AND case_id=$2 ORDER BY case_version DESC",&[&actor.tenant_id,&id]).await?);
    record["approvals"]=json!(array(db,"SELECT to_jsonb(a)-'request_digest'-'idempotency_key' FROM approvals a WHERE tenant_id=$1 AND case_id=$2 ORDER BY created_at DESC",&[&actor.tenant_id,&id]).await?);
    let session = row["session_id"].as_str().unwrap();
    let claim = agent_sessions::claim(db, &actor, session).await?;
    let memories =
        objects::recalled_memories(db, &actor, row["input"].as_str().unwrap_or("")).await?;
    let token = claim["storage_token"].as_str().unwrap();
    let (mcp_session, mcp_token) = mcp_sessions::insert(db, &actor, Some(id), Some(mid)).await?;
    let base = env("DEEPSEEK_BASE_URL", "https://api.deepseek.com");
    let settings: crate::integrations::Settings = serde_json::from_value(row["settings"].clone())?;
    settings.validate()?;
    let model = settings.model.clone();
    ensure!(
        base == "https://api.deepseek.com"
            && ["deepseek-flash", "deepseek-v4-pro"].contains(&model.as_str()),
        "Unsupported model configuration"
    );
    let mut images = Vec::<(String, String)>::new();
    let mut attachment_notes = Vec::<String>::new();
    if row["resumable"] != true {
        let expected: i64 = db.query_one("SELECT count(*) FROM conversation_message_attachments WHERE tenant_id=$1 AND owner_id=$2 AND case_id=$3 AND message_id=$4", &[&actor.tenant_id,&actor.user_id,&id,&mid]).await?.get(0);
        let attachments = db.query("SELECT d.filename,d.content_type,d.object_key,a.source_sha256,d.bytes,d.id FROM conversation_message_attachments a JOIN documents d ON d.id=a.document_id AND d.tenant_id=a.tenant_id AND d.owner_id=a.owner_id AND d.case_id=a.case_id WHERE a.tenant_id=$1 AND a.owner_id=$2 AND a.case_id=$3 AND a.message_id=$4 AND d.state='ready' ORDER BY a.ordinal", &[&actor.tenant_id,&actor.user_id,&id,&mid]).await?;
        ensure!(
            attachments.len() == expected as usize,
            "Attachment is no longer ready"
        );
        let mut total = 0usize;
        for attachment in attachments {
            let filename: String = attachment.get(0);
            let content_type: String = attachment.get(1);
            let key: String = attachment.get(2);
            let hash: String = attachment.get(3);
            let size: i32 = attachment.get(4);
            let document_id: uuid::Uuid = attachment.get(5);
            total += size as usize;
            ensure!(total <= 5 * 1024 * 1024, "Attachment input limit");
            let bucket = app
                .bucket
                .as_ref()
                .context("Document storage unavailable")?;
            let bytes = objects::get_verified(bucket, &key, &hash).await?;
            ensure!(bytes.len() == size as usize, "Attachment size changed");
            if content_type.starts_with("image/") {
                ensure!(
                    model == "deepseek-flash",
                    "Image input requires DeepSeek Flash"
                );
                images.push((content_type, STANDARD.encode(bytes)));
                attachment_notes.push(format!(
                    "Image attachment {}: {} (visible in this turn).",
                    document_id, filename
                ));
            } else {
                let extracted = objects::read_document(
                    &content_type,
                    bytes,
                    model == "deepseek-flash",
                    0,
                    1,
                    4,
                    0,
                )
                .await?;
                let mut warnings = extracted.warnings;
                for image in extracted.images {
                    if images.len() < 4 {
                        images.push((image.mime_type, STANDARD.encode(image.bytes)));
                    } else {
                        warnings.push(format!(
                            "{} was not sent in this turn; use attachment_read.",
                            image.label
                        ));
                    }
                }
                attachment_notes.push(
                    json!({
                        "document_id":document_id,
                        "filename":filename,
                        "source_content_type":content_type,
                        "untrusted_document_text":extracted.text,
                        "next_offset":extracted.next_offset,
                        "next_page":extracted.next_page,
                        "next_image_offset":extracted.next_image_offset,
                        "complete":extracted.complete && warnings.is_empty(),
                        "warnings":warnings,
                        "continuation_tool":"attachment_read"
                    })
                    .to_string(),
                );
            }
        }
    }
    let binary = std::fs::canonicalize(env(
        "AGENT_RUNTIME_BINARY",
        "services/runtime-bridge/target/debug/erp-agent-runtime",
    ))?;
    let dir = tempfile::Builder::new()
        .prefix("erp-agent-turn-")
        .tempdir()?;
    let home = dir.path();
    std::fs::create_dir(home.join("work"))?;
    let origin = env(
        "AGENT_STORAGE_ORIGIN",
        &format!("http://127.0.0.1:{}", app.port),
    );
    let mut command = tokio::process::Command::new(binary);
    let turn_timeout: u64 = env("AGENT_TURN_TIMEOUT_SECONDS", "0").parse()?;
    ensure!(turn_timeout <= 3600, "Invalid turn timeout");
    let gateway = std::fs::canonicalize(env(
        "AGENT_MCP_BINARY",
        "services/mcp-gateway/target/debug/erp-agent-mcp",
    ))?;
    command
        .current_dir(home.join("work"))
        .env_clear()
        .envs([
            ("PATH", "/usr/bin:/bin"),
            ("LANG", "C.UTF-8"),
            (
                "OPENAI_API_KEY",
                app.model_key
                    .as_deref()
                    .context("Model credential missing")?,
            ),
            ("JCODE_OPENAI_MODEL", &model),
            ("JCODE_OPENAI_API_BASE", &base),
            ("JCODE_OPENAI_TRANSPORT", "https"),
            ("JCODE_OPENAI_NATIVE_COMPACTION_MODE", "off"),
            ("JCODE_OPENAI_MAX_OUTPUT_TOKENS", "4096"),
            ("JCODE_MAX_RETRIES", "1"),
            ("JCODE_MEMORY_ENABLED", "false"),
            ("JCODE_MEMORY_SIDECAR_ENABLED", "false"),
            ("JCODE_NO_TELEMETRY", "1"),
            ("DO_NOT_TRACK", "1"),
            ("JCODE_PRESERVE_REASONING_CONTEXT", "true"),
            ("JCODE_SESSION_STORE_URL", &origin),
            ("JCODE_SESSION_STORE_ID", session),
            ("JCODE_SESSION_STORE_TOKEN", token),
            ("JCODE_MCP_SESSION_ID", &mcp_session),
            ("JCODE_MCP_SESSION_TOKEN", &mcp_token),
        ])
        .env("HOME", home)
        .env("AGENT_MCP_BINARY", gateway)
        .env("AGENT_TURN_TIMEOUT_SECONDS", turn_timeout.to_string())
        .env("TMPDIR", home)
        .env("JCODE_HOME", home.join("jcode"))
        .env("XDG_CONFIG_HOME", home.join("config"));
    let soft_ratio: f32 = env("AGENT_COMPACTION_SOFT_RATIO", "0.80").parse()?;
    let critical_ratio: f32 = env("AGENT_COMPACTION_CRITICAL_RATIO", "0.90").parse()?;
    ensure!(
        soft_ratio.is_finite()
            && critical_ratio.is_finite()
            && 0.0 < soft_ratio
            && soft_ratio < critical_ratio
            && critical_ratio < 1.0,
        "Invalid compaction thresholds"
    );
    command.env("AGENT_COMPACTION_SOFT_RATIO", soft_ratio.to_string());
    command.env(
        "AGENT_COMPACTION_CRITICAL_RATIO",
        critical_ratio.to_string(),
    );
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    timing.mark("runtime_started");
    let permission_context = json!({
        "mode": settings.approval,
        "operation_tools": ["memory_save", "case_set_context", "proposal_create"],
        "read_tools": [
            "case_read", "attachment_read", "cube_quantity_summary", "cube_browser_browse", "todo", "skill_find", "skill_manage",
            "mcp__erp__list_erp_orders", "mcp__erp__read_erp_order",
            "mcp__openviking__search_case_knowledge", "mcp__openviking__read_knowledge_source"
        ],
        "business_approval_tool": "request_proposal_approval",
        "can_approve_business_proposals": actor.roles.iter().any(|role| role == "approver"),
        "business_approval_requires_human": true
    });
    let user_text = row["input"].as_str().unwrap_or("");
    let mut turn_text = if user_text.is_empty() {
        "Please review the attached material.".to_owned()
    } else {
        user_text.to_owned()
    };
    if !attachment_notes.is_empty() {
        turn_text.push_str("\n\nAttached material status:\n");
        turn_text.push_str(&attachment_notes.join("\n\n"));
    }
    let input = json!({"text":turn_text,"images":images,"new_session":claim["revision"]==0,"case":record,"operation":row["operation"],"memories":memories,"approval":settings.approval,"permission_context":permission_context,"model":settings.model,"effort":settings.effort,"resume":row["resumable"]==true});
    timing.mark("input_ready");
    let mut stdin = child.stdin.take().unwrap();
    stdin.write_all(&serde_json::to_vec(&input)?).await?;
    stdin.shutdown().await?;
    drop(stdin);
    let stderr = child.stderr.take().unwrap();
    let diagnostics = tokio::spawn(async move {
        let mut bytes = Vec::new();
        let _ = stderr.take(32768).read_to_end(&mut bytes).await;
        bytes
    });
    let mut output = BufReader::new(child.stdout.take().unwrap().take(2 * 1024 * 1024));
    let mut line = Vec::new();
    let mut completed = false;
    let mut runtime_failure = None;
    let mut stopping_at = None;
    let mut first_token_seen = row["first_token_at"].as_str().is_some();
    let pid = child.id().context("Runtime PID missing")?;
    let mut snapshot: Option<Value> = None;
    let mut last_save = Instant::now();
    let mut pulse = tokio::time::interval(Duration::from_secs(1));
    let work = async {
        loop {
            line.clear();
            let count = loop {
                tokio::select! {
                    result=output.read_until(b'\n',&mut line)=>break result?,
                    _=pulse.tick()=>{
                        let cancelled: bool=db.query_one("SELECT cancel_requested FROM agent_runs WHERE message_id=$1",&[&mid]).await?.get(0);
                        if (cancelled || stop.is_cancelled()) && stopping_at.is_none() {
                            // Keep the scoped storage lease valid while JCode saves
                            // its partial response and closes outstanding tool calls.
                            unsafe { libc::kill(pid as libc::pid_t, libc::SIGTERM); }
                            stopping_at = Some(Instant::now());
                        }
                        if stopping_at.is_some_and(|at| at.elapsed() > Duration::from_secs(30)) {
                            anyhow::bail!(TurnFailure::Interrupted);
                        }
                    }
                }
            };
            if count == 0 {
                break;
            }
            ensure!(
                line.len() <= 128 * 1024 && !completed,
                "Invalid runtime stream"
            );
            let event: Value = serde_json::from_slice(&line)?;
            let kind = event["type"].as_str().context("Event missing")?;
            timing.event(&event);
            if !first_token_seen
                && ["text_delta", "text_replace"].contains(&kind)
                && event["text"].as_str().is_some_and(|text| !text.is_empty())
            {
                db.execute("UPDATE agent_runs SET first_token_at=COALESCE(first_token_at,clock_timestamp()) WHERE message_id=$1", &[&mid]).await.context("recording first output token")?;
                first_token_seen = true;
            }
            match kind {
                "skill_catalog" => {
                    ensure!(snapshot.is_none(), "Late catalog");
                    let source = event["snapshot"].as_str().context("Catalog missing")?;
                    let hash = event["hash"].as_str().context("Catalog hash missing")?;
                    snapshot = Some(catalog(source, hash)?);
                    db.execute("INSERT INTO agent_skill_catalogs(hash,snapshot) VALUES($1,$2) ON CONFLICT(hash) DO NOTHING",&[&hash,&source]).await?;
                    db.execute(
                        "UPDATE agent_runs SET skill_catalog_hash=$2 WHERE message_id=$1",
                        &[&mid, &hash],
                    )
                    .await?;
                }
                "skill_loaded" => {
                    let found = snapshot
                        .as_ref()
                        .and_then(|s| s["skills"].as_array())
                        .is_some_and(|skills| {
                            skills.iter().any(|s| {
                                s["id"] == event["id"]
                                    && s["version"] == event["version"]
                                    && s["sha256"] == event["sha256"]
                            })
                        });
                    ensure!(found, "Loaded skill differs from catalog");
                    let reference = json!([{ "id":event["id"],"version":event["version"],"sha256":event["sha256"] }]);
                    db.execute("UPDATE agent_runs SET loaded_skills=CASE WHEN loaded_skills @> $2::jsonb THEN loaded_skills ELSE loaded_skills || $2::jsonb END WHERE message_id=$1",&[&mid,&reference]).await?;
                }
                "failed" => {
                    if let Some(detail) = event["detail"].as_str() {
                        eprintln!(
                            "Cube runtime failure detail: code={} detail={}",
                            event["code"].as_str().unwrap_or("runtime"),
                            detail
                        );
                    }
                    runtime_failure = Some(match event["code"].as_str() {
                        Some("timeout") => TurnFailure::Timeout,
                        Some("interrupted") => TurnFailure::Interrupted,
                        _ => TurnFailure::Runtime,
                    });
                    completed = true;
                }
                "suspended" => {
                    reply.suspended = true;
                    completed = true;
                    reply.close("completed");
                }
                _ => {
                    ensure!(snapshot.is_some(), "Model activity before catalog");
                    reply.event(&event)?;
                    if kind == "completed" {
                        completed = true
                    }
                }
            }
            if last_save.elapsed() > Duration::from_millis(120)
                || !["text_delta", "reasoning_delta"].contains(&kind)
            {
                reply.save(db, mid).await?;
                last_save = Instant::now();
            }
        }
        let status = child.wait().await?;
        if let Some(error) = runtime_failure.take() {
            return Err(error.into());
        }
        if stopping_at.is_some() {
            return Err(TurnFailure::Interrupted.into());
        }
        ensure!(
            status.success() && completed,
            "Runtime ended without completion"
        );
        Ok::<_, anyhow::Error>(())
    };
    let result = if turn_timeout == 0 {
        work.await
    } else {
        tokio::time::timeout(Duration::from_secs(turn_timeout + 35), work)
            .await
            .map_err(|_| anyhow::Error::new(TurnFailure::Timeout))
            .and_then(|r| r)
    };
    if result.is_err() {
        let _ = child.kill().await;
    }
    let bytes = diagnostics.await.unwrap_or_default();
    if result.is_err() {
        let message = String::from_utf8_lossy(&bytes)
            .replace(app.model_key.as_deref().unwrap_or("<none>"), "[redacted]")
            .replace(token, "[redacted]");
        eprintln!(
            "Agent run {mid}: {}",
            message.chars().take(3000).collect::<String>()
        );
    }
    result
}

async fn turn_cube(
    app: &App,
    db: &tokio_postgres::Client,
    row: &Value,
    reply: &mut Reply,
    stop: &tokio_util::sync::CancellationToken,
    timing: &mut RunTiming,
) -> anyhow::Result<()> {
    ensure!(!stop.is_cancelled(), "Run interrupted");
    let actor = Principal {
        tenant_id: uuid(row["tenant_id"].as_str().unwrap())?,
        user_id: uuid(row["actor_id"].as_str().unwrap())?,
        roles: serde_json::from_value(row["roles"].clone())?,
        email: String::new(),
    };
    let case_id = uuid(row["case_id"].as_str().unwrap())?;
    let message_id = row["message_id"].as_i64().unwrap();
    ensure!(
        !crate::sandboxes::switch_pending(db, actor.tenant_id, case_id).await?,
        "Runtime template switch in progress"
    );
    // Cube is an internal execution detail. The business UI never manages a
    // sandbox, so the first queued turn provisions the resident instance and
    // later turns reuse it through the case-bound database lease.
    let mut ready_timing = crate::sandboxes::ReadyTiming::default();
    let ready =
        crate::sandboxes::ensure_resident_ready(app, &actor, case_id, &mut ready_timing).await;
    timing.lifecycle = Some(json!(ready_timing));
    ready?;
    timing.mark("sandbox_ready");
    let mut record = cases::case(db, &actor, case_id, false).await?;
    record["proposals"] = json!(array(
        db,
        "SELECT to_jsonb(p) FROM proposals p WHERE tenant_id=$1 AND case_id=$2 ORDER BY case_version DESC",
        &[&actor.tenant_id, &case_id]
    )
    .await?);
    record["approvals"] = json!(array(
        db,
        "SELECT to_jsonb(a)-'request_digest'-'idempotency_key' FROM approvals a WHERE tenant_id=$1 AND case_id=$2 ORDER BY created_at DESC",
        &[&actor.tenant_id, &case_id]
    )
    .await?);

    let sandbox = db
        .query_opt(
            "SELECT to_jsonb(s) FROM case_sandboxes s WHERE tenant_id=$1 AND case_id=$2 AND active=true AND state='provisioned' ORDER BY created_at DESC LIMIT 1",
            &[&actor.tenant_id, &case_id],
        )
        .await?
        .context("Cube sandbox is not provisioned")?;
    let sandbox: Value = sandbox.get(0);
    ensure!(sandbox["guest_ready"] == true, "Cube runtime is not ready");
    let sandbox_id = sandbox["sandbox_id"]
        .as_str()
        .context("Cube sandbox id missing")?;
    let operation = uuid(
        sandbox["id"]
            .as_str()
            .context("Cube operation id missing")?,
    )?;

    let settings: crate::integrations::Settings = serde_json::from_value(row["settings"].clone())?;
    settings.validate()?;
    let base = env("DEEPSEEK_BASE_URL", "https://api.deepseek.com");
    ensure!(
        base == "https://api.deepseek.com"
            && ["deepseek-flash", "deepseek-v4-pro"].contains(&settings.model.as_str()),
        "Unsupported model configuration"
    );
    let memories =
        objects::recalled_memories(db, &actor, row["input"].as_str().unwrap_or("")).await?;
    let session_id = row["session_id"]
        .as_str()
        .context("Agent session missing")?;
    let revision: i64 = db
        .query_one(
            "SELECT revision::bigint FROM agent_sessions WHERE id=$1 AND tenant_id=$2 AND case_id=$3",
            &[&session_id, &actor.tenant_id, &case_id],
        )
        .await?
        .get(0);

    let mut images = Vec::<(String, String)>::new();
    let mut attachment_notes = Vec::<String>::new();
    if row["resumable"] != true {
        let expected: i64 = db
            .query_one(
                "SELECT count(*) FROM conversation_message_attachments WHERE tenant_id=$1 AND owner_id=$2 AND case_id=$3 AND message_id=$4",
                &[&actor.tenant_id, &actor.user_id, &case_id, &message_id],
            )
            .await?
            .get(0);
        let attachments = db
            .query(
                "SELECT d.filename,d.content_type,d.object_key,a.source_sha256,d.bytes,d.id FROM conversation_message_attachments a JOIN documents d ON d.id=a.document_id AND d.tenant_id=a.tenant_id AND d.owner_id=a.owner_id AND d.case_id=a.case_id WHERE a.tenant_id=$1 AND a.owner_id=$2 AND a.case_id=$3 AND a.message_id=$4 AND d.state='ready' ORDER BY a.ordinal",
                &[&actor.tenant_id, &actor.user_id, &case_id, &message_id],
            )
            .await?;
        ensure!(
            attachments.len() == expected as usize,
            "Attachment is no longer ready"
        );
        let mut total = 0usize;
        for attachment in attachments {
            let filename: String = attachment.get(0);
            let content_type: String = attachment.get(1);
            let key: String = attachment.get(2);
            let hash: String = attachment.get(3);
            let size: i32 = attachment.get(4);
            let document_id: uuid::Uuid = attachment.get(5);
            total += size as usize;
            ensure!(total <= 5 * 1024 * 1024, "Attachment input limit");
            let bucket = app
                .bucket
                .as_ref()
                .context("Document storage unavailable")?;
            let bytes = objects::get_verified(bucket, &key, &hash).await?;
            ensure!(bytes.len() == size as usize, "Attachment size changed");
            if content_type.starts_with("image/") {
                ensure!(
                    settings.model == "deepseek-flash",
                    "Image input requires DeepSeek Flash"
                );
                images.push((content_type, STANDARD.encode(bytes)));
                attachment_notes.push(format!(
                    "Image attachment {}: {} (visible in this turn).",
                    document_id, filename
                ));
            } else {
                let extracted = objects::read_document(
                    &content_type,
                    bytes,
                    settings.model == "deepseek-flash",
                    0,
                    1,
                    4,
                    0,
                )
                .await?;
                let mut warnings = extracted.warnings;
                for image in extracted.images {
                    if images.len() < 4 {
                        images.push((image.mime_type, STANDARD.encode(image.bytes)));
                    } else {
                        warnings.push(format!(
                            "{} was not sent in this turn; use attachment_read.",
                            image.label
                        ));
                    }
                }
                attachment_notes.push(
                    json!({
                        "document_id":document_id,
                        "filename":filename,
                        "source_content_type":content_type,
                        "untrusted_document_text":extracted.text,
                        "next_offset":extracted.next_offset,
                        "next_page":extracted.next_page,
                        "next_image_offset":extracted.next_image_offset,
                        "complete":extracted.complete && warnings.is_empty(),
                        "warnings":warnings,
                        "continuation_tool":"attachment_read"
                    })
                    .to_string(),
                );
            }
        }
    }

    let permission_context = json!({
        "mode": settings.approval,
        "operation_tools": ["memory_save", "case_set_context", "proposal_create"],
        "read_tools": [
            "case_read", "attachment_read", "cube_quantity_summary", "cube_browser_browse", "todo", "skill_find", "skill_manage",
            "mcp__erp__list_erp_orders", "mcp__erp__read_erp_order",
            "mcp__openviking__search_case_knowledge", "mcp__openviking__read_knowledge_source"
        ],
        "business_approval_tool": "request_proposal_approval",
        "can_approve_business_proposals": actor.roles.iter().any(|role| role == "approver"),
        "business_approval_requires_human": true
    });
    let user_text = row["input"].as_str().unwrap_or("");
    let mut turn_text = if user_text.is_empty() {
        "Please review the attached material.".to_owned()
    } else {
        user_text.to_owned()
    };
    if !attachment_notes.is_empty() {
        turn_text.push_str("\n\nAttached material status:\n");
        turn_text.push_str(&attachment_notes.join("\n\n"));
    }
    let input = json!({
        "text": turn_text,
        "images": images,
        "new_session": revision == 0,
        "case": record,
        "operation": row["operation"],
        "memories": memories,
        "approval": settings.approval,
        "permission_context": permission_context,
        "model": settings.model,
        "effort": settings.effort,
        "resume": row["resumable"] == true
    });
    timing.mark("input_ready");
    ensure!(!stop.is_cancelled(), "Run interrupted");
    let url = crate::sandboxes::bridge::guest_url(sandbox_id, "/v1/turn")?;
    let token = crate::sandboxes::bridge::token_for(operation)?;
    let configured_timeout: u64 = env("AGENT_TURN_TIMEOUT_SECONDS", "0").parse()?;
    ensure!(configured_timeout <= 3600, "Invalid turn timeout");
    let mut request = app
        .http
        .post(url)
        .header("x-agent-runtime-token", token)
        .json(&input);
    if configured_timeout > 0 {
        request = request.timeout(Duration::from_secs(configured_timeout.saturating_add(35)));
    }
    timing.mark("bridge_request");
    let mut response = request
        .send()
        .await
        .context("Cube runtime request failed")?;
    timing.mark("bridge_response");
    let status = response.status();
    if !status.is_success() {
        let body = response
            .bytes()
            .await
            .context("Reading Cube runtime failure")?;
        anyhow::bail!(
            "Cube runtime returned {status}: {}",
            String::from_utf8_lossy(&body[..body.len().min(1024)])
        );
    }

    let mut snapshot = if let Some(hash) = row["skill_catalog_hash"].as_str() {
        db.query_opt(
            "SELECT snapshot FROM agent_skill_catalogs WHERE hash=$1",
            &[&hash],
        )
        .await?
        .map(|value| {
            let source: String = value.get(0);
            catalog(&source, hash)
        })
        .transpose()?
    } else {
        None
    };
    let mut completed = false;
    let mut runtime_failure = None;
    let mut first_token_seen = row["first_token_at"].as_str().is_some();
    let mut lines = CubeLines::default();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("Reading Cube runtime stream")?
    {
        for event in lines.push(&chunk)? {
            ensure!(!completed, "Invalid Cube runtime stream");
            let kind = event["type"].as_str().context("Cube event type missing")?;
            timing.event(&event);
            if !first_token_seen
                && ["text_delta", "text_replace"].contains(&kind)
                && event["text"].as_str().is_some_and(|text| !text.is_empty())
            {
                db.execute(
                "UPDATE agent_runs SET first_token_at=COALESCE(first_token_at,clock_timestamp()) WHERE message_id=$1",
                &[&message_id],
            )
            .await?;
                first_token_seen = true;
            }
            match kind {
                "skill_catalog" => {
                    ensure!(snapshot.is_none(), "Late catalog");
                    let source = event["snapshot"].as_str().context("Catalog missing")?;
                    let hash = event["hash"].as_str().context("Catalog hash missing")?;
                    snapshot = Some(catalog(source, hash)?);
                    db.execute(
                    "INSERT INTO agent_skill_catalogs(hash,snapshot) VALUES($1,$2) ON CONFLICT(hash) DO NOTHING",
                    &[&hash, &source],
                )
                .await?;
                    db.execute(
                        "UPDATE agent_runs SET skill_catalog_hash=$2 WHERE message_id=$1",
                        &[&message_id, &hash],
                    )
                    .await?;
                }
                "runtime_timing" => {
                    ensure!(snapshot.is_some(), "Runtime timing before catalog");
                    ensure!(
                        event["reused"].is_boolean()
                            && event["build_ms"].is_u64()
                            && event["model_catalog_ms"].is_u64(),
                        "Invalid runtime timing"
                    );
                }
                "skill_loaded" => {
                    let found = snapshot
                        .as_ref()
                        .and_then(|value| value["skills"].as_array())
                        .is_some_and(|skills| {
                            skills.iter().any(|skill| {
                                skill["id"] == event["id"]
                                    && skill["version"] == event["version"]
                                    && skill["sha256"] == event["sha256"]
                            })
                        });
                    ensure!(found, "Loaded skill differs from catalog");
                    let reference = json!([{
                        "id":event["id"],
                        "version":event["version"],
                        "sha256":event["sha256"]
                    }]);
                    db.execute(
                    "UPDATE agent_runs SET loaded_skills=CASE WHEN loaded_skills @> $2::jsonb THEN loaded_skills ELSE loaded_skills || $2::jsonb END WHERE message_id=$1",
                    &[&message_id, &reference],
                )
                .await?;
                }
                "failed" => {
                    if let Some(detail) = event["detail"].as_str() {
                        eprintln!("Cube runtime failure detail: {}", detail);
                    }
                    runtime_failure = Some(match event["code"].as_str() {
                        Some("timeout") => TurnFailure::Timeout,
                        Some("interrupted") => TurnFailure::Interrupted,
                        _ => TurnFailure::Runtime,
                    });
                    completed = true;
                }
                "suspended" => {
                    reply.suspended = true;
                    completed = true;
                    reply.close("completed");
                }
                _ => {
                    ensure!(snapshot.is_some(), "Model activity before catalog");
                    reply.event(&event)?;
                    if kind == "completed" {
                        completed = true;
                    }
                }
            }
            reply.save(db, message_id).await?;
        }
    }
    lines.finish()?;
    if let Some(error) = runtime_failure {
        return Err(error.into());
    }
    ensure!(completed, "Cube runtime ended without completion");
    reply.save(db, message_id).await?;
    Ok(())
}

async fn candidate_ids(app: &App) -> anyhow::Result<Vec<(i64, uuid::Uuid, String)>> {
    let db = app.db.get().await?;
    let rows = db.query("SELECT message_id,case_id,state FROM agent_runs WHERE state IN ('queued','running','waiting') ORDER BY CASE state WHEN 'queued' THEN 0 WHEN 'running' THEN 1 ELSE 2 END,updated_at LIMIT 100",&[]).await?;
    Ok(rows
        .into_iter()
        .map(|row| (row.get(0), row.get(1), row.get(2)))
        .collect())
}

async fn listen_once(app: &App, stop: &tokio_util::sync::CancellationToken) -> anyhow::Result<()> {
    let mut config: tokio_postgres::Config = std::env::var("DATABASE_URL")?.parse()?;
    config.connect_timeout(Duration::from_secs(5));
    let (client, mut connection) = config.connect(NoTls).await?;
    let wake = app.agent_wake.clone();
    let mut messages = tokio::spawn(async move {
        loop {
            match poll_fn(|cx| connection.poll_message(cx)).await {
                Some(Ok(AsyncMessage::Notification(_))) => wake.notify_one(),
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
                None => anyhow::bail!("Agent notification connection closed"),
            }
        }
    });
    if let Err(error) = client.batch_execute("LISTEN erp_agent_runs").await {
        messages.abort();
        return Err(error.into());
    }
    app.agent_wake.notify_one();
    tokio::select! {
        _ = stop.cancelled() => {
            messages.abort();
            let _ = messages.await;
            Ok(())
        }
        result = &mut messages => result??,
    }
}

async fn listen_for_runs(app: App, stop: tokio_util::sync::CancellationToken) {
    while !stop.is_cancelled() {
        if let Err(error) = listen_once(&app, &stop).await {
            eprintln!("Agent notifications: {error}");
            tokio::select! {
                _ = stop.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(2)) => {}
            }
        }
    }
}

async fn process_candidate(
    app: &App,
    id: i64,
    case: uuid::Uuid,
    stop: &tokio_util::sync::CancellationToken,
) -> anyhow::Result<()> {
    if stop.is_cancelled() {
        return Ok(());
    }
    let mut db = app.db.get().await?;
    let key = format!("agent_run:{case}");
    let locked: bool = db
        .query_one(
            "SELECT pg_try_advisory_lock(hashtextextended($1,0))",
            &[&key],
        )
        .await?
        .get(0);
    if !locked {
        return Ok(());
    }
    let result=async{
            let row=db.query_opt("SELECT to_jsonb(r)||jsonb_build_object('input',m.text,'actor_id',m.actor_id,'session_id',c.session_id,'active',u.active,'roles',u.roles) FROM agent_runs r JOIN case_conversation_messages m ON m.id=r.message_id JOIN case_conversations c ON c.tenant_id=r.tenant_id AND c.case_id=r.case_id JOIN users u ON u.id=m.actor_id AND u.tenant_id=r.tenant_id WHERE r.message_id=$1 AND r.state IN ('queued','running','waiting')",&[&id]).await?;
            let Some(row)=row else{return Ok::<_,anyhow::Error>(())};let row:Value=row.get(0);
            if row["cancel_requested"]==true||(row["state"]=="running" && row["resumable"]!=true)||row["active"]!=true||!row["roles"].as_array().is_some_and(|rs|rs.iter().any(|r|r=="planner")){
                let tx=db.transaction().await?;
                if !crate::sandboxes::bridge::enabled() {
                    tx.execute("UPDATE agent_sessions SET token_hash=NULL,expires_at=NULL WHERE id=$1",&[&row["session_id"].as_str().unwrap()]).await?;
                }
                let error = if row["operation"] == "compact" { "Context compression was interrupted. You can retry compression." } else { "Run interrupted. Send a new message to continue." };
                tx.execute("UPDATE agent_runs SET state='failed',error=$2,tools=COALESCE((SELECT jsonb_agg(CASE WHEN tool->>'state'='running' THEN tool||'{\"state\":\"failed\"}'::jsonb ELSE tool END) FROM jsonb_array_elements(tools) tool),'[]'::jsonb),steps=COALESCE((SELECT jsonb_agg(CASE WHEN step->>'state'='running' THEN step||'{\"state\":\"failed\"}'::jsonb ELSE step END) FROM jsonb_array_elements(steps) step),'[]'::jsonb),finished_at=now(),updated_at=now() WHERE message_id=$1",&[&id,&error]).await?;
                if row["cancel_requested"]==true {tx.execute("UPDATE agent_runs SET state='cancelled',error=NULL WHERE message_id=$1", &[&id]).await?;}
                tx.execute("UPDATE agent_tool_permissions SET state='cancelled',decided_by='cancel',reason='会话已中断',decided_at=now() WHERE message_id=$1 AND state IN ('pending','allowed')", &[&id]).await?;
                tx.commit().await?;return Ok(())
            }
            if row["state"]=="waiting" || row["state"]=="running" {
                if !crate::sandboxes::bridge::enabled() {
                    db.execute("UPDATE agent_sessions SET token_hash=NULL,expires_at=NULL WHERE id=$1", &[&row["session_id"].as_str().unwrap()]).await?;
                }
                db.execute("UPDATE agent_runs SET state='waiting',updated_at=now() WHERE message_id=$1 AND state IN ('running','waiting')", &[&id]).await?;
                crate::permissions::resolve_waiting(app,id).await?;
                return Ok(());
            }
            if db.execute("UPDATE agent_runs SET state='running',started_at=now(),updated_at=now() WHERE message_id=$1 AND state='queued' AND NOT cancel_requested",&[&id]).await?==0{return Ok(());}
            let mut timing = RunTiming::new(if crate::sandboxes::bridge::enabled() { "cube" } else { "host" });
            let mut reply=if row["resumable"]==true { Reply { text:row["text"].as_str().unwrap_or("").into(), tools:serde_json::from_value(row["tools"].clone())?,steps:serde_json::from_value(row["steps"].clone())?,compactions:serde_json::from_value(row["compactions"].clone())?,context_usage:row.get("context_usage").filter(|value| !value.is_null()).cloned(),boundary:true,suspended:false } } else { Reply::default() };
            let result=turn(app,&db,&row,&mut reply,stop,&mut timing).await;
            timing.mark("turn_returned");
            if !crate::sandboxes::bridge::enabled() {
                db.execute("UPDATE mcp_sessions SET revoked_at=now(),updated_at=now() WHERE run_message_id=$1 AND revoked_at IS NULL", &[&id]).await?;
            }
            let recoverable = stop.is_cancelled() && db.query_one("SELECT resumable FROM agent_runs WHERE message_id=$1", &[&id]).await?.get::<_,bool>(0);
            if let Err(error)=&result {if !recoverable {eprintln!("Agent run {id} failed: {error:#}");reply.close("failed");for t in &mut reply.tools{if t["state"]=="running"{t["state"]=json!("failed")}}}}
            reply.save(&db,id).await?;
            let tx=db.transaction().await?;
            if !crate::sandboxes::bridge::enabled() {
                tx.execute("UPDATE agent_sessions SET token_hash=NULL,expires_at=NULL WHERE id=$1",&[&row["session_id"].as_str().unwrap()]).await?;
            }
            let cancelled: bool = tx.query_one("SELECT cancel_requested FROM agent_runs WHERE message_id=$1 FOR UPDATE",&[&id]).await?.get(0);
            let error = if cancelled || recoverable { None } else { result.as_ref().err().map(|error| if row["operation"] == "compact" { "Context compression failed. You can retry compression." } else { match error.downcast_ref::<TurnFailure>() {
                Some(TurnFailure::Timeout) => "Generation reached the time limit. Send a message to continue from the saved content.",
                Some(TurnFailure::Interrupted) => "Generation was interrupted. Send a message to continue from the saved content.",
                _ => "Generation stopped unexpectedly. Your saved content is retained; send a message to continue.",
            } }) };
            let pending: bool = tx.query_one("SELECT EXISTS(SELECT 1 FROM agent_tool_permissions WHERE message_id=$1 AND state='pending')", &[&id]).await?.get(0);
            let state = if cancelled {"cancelled"} else if recoverable {"waiting"} else if result.is_err() {"failed"} else if reply.suspended {if pending {"waiting"}else{"queued"}} else {"completed"};
            let timing_sample = timing.snapshot();
            tx.execute("UPDATE agent_runs SET state=$2,error=$3,finished_at=CASE WHEN $2 IN ('completed','failed','cancelled') THEN now() END,updated_at=now(),timings=timings||jsonb_build_array($4::jsonb) WHERE message_id=$1",&[&id,&state,&error,&timing_sample]).await?;
            if ["cancelled","failed"].contains(&state) {
                tx.execute("UPDATE agent_tool_permissions SET state='cancelled',decided_by='cancel',reason='会话已停止',decided_at=now() WHERE message_id=$1 AND state IN ('pending','allowed')", &[&id]).await?;
            }
            tx.commit().await?;Ok(())
        }.await;
    if db
        .query_one("SELECT pg_advisory_unlock(hashtextextended($1,0))", &[&key])
        .await
        .is_err()
    {
        drop(deadpool_postgres::Object::take(db));
    }
    result
}
pub fn max_concurrent_runs() -> anyhow::Result<usize> {
    let value = env("AGENT_MAX_CONCURRENT_RUNS", "4").parse::<usize>()?;
    anyhow::ensure!(
        (1..=8).contains(&value),
        "AGENT_MAX_CONCURRENT_RUNS must be between 1 and 8"
    );
    Ok(value)
}

pub async fn run(app: App, stop: Arc<tokio_util::sync::CancellationToken>, concurrency: usize) {
    if app.model_key.is_none() {
        return;
    }
    let mut tasks = tokio::task::JoinSet::new();
    let mut in_flight = HashSet::new();
    let listener = tokio::spawn(listen_for_runs(app.clone(), stop.as_ref().clone()));
    let mut poll = tokio::time::interval(Duration::from_secs(5));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut backfill = None;
    loop {
        let mut check_backfill = false;
        tokio::select! {
            _ = stop.cancelled() => break,
            done = tasks.join_next(), if !tasks.is_empty() => {
                match done {
                    Some(Ok((case, result))) => {
                        in_flight.remove(&case);
                        if let Err(error) = result { eprintln!("Agent worker: {error}"); }
                    }
                    Some(Err(error)) => {
                        eprintln!("Agent worker task: {error}");
                        in_flight.clear();
                    }
                    None => {}
                }
            }
            _ = app.agent_wake.notified() => { check_backfill = true; }
            _ = poll.tick() => { check_backfill = true; }
            _ = async {
                if let Some(deadline) = backfill {
                    tokio::time::sleep_until(deadline).await;
                }
            }, if backfill.is_some() => { backfill = None; }
        }
        if stop.is_cancelled() {
            break;
        }
        let mut queued_candidates = false;
        if tasks.len() < concurrency {
            match candidate_ids(&app).await {
                Ok(mut candidates) => {
                    queued_candidates = candidates.iter().any(|candidate| candidate.2 == "queued");
                    // Different workers otherwise race for the same head of the queue.
                    let queued = candidates.partition_point(|candidate| candidate.2 == "queued");
                    if queued > 1 {
                        let offset = uuid::Uuid::new_v4().as_u128() as usize % queued;
                        candidates[..queued].rotate_left(offset);
                    }
                    for (id, case, _) in candidates {
                        if tasks.len() >= concurrency {
                            break;
                        }
                        if !in_flight.insert(case) {
                            continue;
                        }
                        let app = app.clone();
                        let stop = stop.as_ref().clone();
                        tasks.spawn(async move {
                            (case, process_candidate(&app, id, case, &stop).await)
                        });
                    }
                }
                Err(error) => eprintln!("Agent worker: {error}"),
            }
        }
        if (check_backfill || queued_candidates) && tasks.len() < concurrency && backfill.is_none()
        {
            backfill = Some(tokio::time::Instant::now() + Duration::from_millis(100));
        }
    }
    while let Some(done) = tasks.join_next().await {
        if let Ok((_, Err(error))) = done {
            eprintln!("Agent worker shutdown: {error}");
        }
    }
    let _ = listener.await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temporary_bash_is_published_only_when_enabled() {
        assert!(!tool_is_published("bash", false));
        assert!(tool_is_published("bash", true));
        assert!(tool_is_published("case_read", false));
        assert!(tool_is_published("mcp__erp__read_erp_order", false));
        assert!(!tool_is_published("sh", true));
    }

    #[test]
    fn cube_lines_decode_events_across_network_chunks() {
        let mut lines = CubeLines::default();
        assert!(lines.push(b"{\"type\":\"text_").unwrap().is_empty());
        assert_eq!(
            lines
                .push(b"delta\",\"text\":\"ok\"}\n{\"type\":\"completed\"}\n")
                .unwrap(),
            vec![
                json!({"type":"text_delta","text":"ok"}),
                json!({"type":"completed"})
            ]
        );
        lines.finish().unwrap();
        assert!(lines.push(b"partial").unwrap().is_empty());
        assert!(lines.finish().is_err());
        assert!(
            CubeLines::default()
                .push(&vec![b'x'; 128 * 1024 + 1])
                .is_err()
        );
    }

    fn valid_catalog() -> (String, String) {
        let source = "# Skill\nDo the bounded task.";
        let value = json!({
            "schema_version":1,"id":"catalog-1",
            "skills":[{"id":"case-review","version":"1.0.0","source":source,"sha256":digest(source)}]
        });
        let snapshot = serde_json::to_string(&value).unwrap();
        let hash = digest(&snapshot);
        (snapshot, hash)
    }

    #[test]
    fn catalog_binds_digest_schema_unique_ids_and_skill_source() {
        let (snapshot, hash) = valid_catalog();
        assert_eq!(catalog(&snapshot, &hash).unwrap()["id"], "catalog-1");
        assert!(catalog(&snapshot, "0").is_err());
        let mut value: Value = serde_json::from_str(&snapshot).unwrap();
        value["schema_version"] = json!(2);
        let changed = serde_json::to_string(&value).unwrap();
        assert!(catalog(&changed, &digest(&changed)).is_err());
        let mut value: Value = serde_json::from_str(&snapshot).unwrap();
        let duplicate = value["skills"][0].clone();
        value["skills"].as_array_mut().unwrap().push(duplicate);
        let changed = serde_json::to_string(&value).unwrap();
        assert!(catalog(&changed, &digest(&changed)).is_err());
        let mut value: Value = serde_json::from_str(&snapshot).unwrap();
        value["skills"][0]["sha256"] = json!("changed");
        let changed = serde_json::to_string(&value).unwrap();
        assert!(catalog(&changed, &digest(&changed)).is_err());
    }

    #[test]
    fn reply_preserves_reasoning_text_tools_compaction_and_boundaries() {
        let mut reply = Reply::default();
        for event in [
            json!({"type":"reasoning_delta","text":"inspect"}),
            json!({"type":"reasoning_delta","text":" data"}),
            json!({"type":"reasoning_done"}),
            json!({"type":"text_delta","text":"first"}),
            json!({"type":"text_done"}),
            json!({"type":"text_delta","text":"second"}),
            json!({"type":"tool_start","id":"tool-1","name":"case_read"}),
            json!({"type":"tool_done","id":"tool-1","name":"case_read","failed":false}),
            json!({"type":"compaction","trigger":"manual","pre_tokens":100,"post_tokens":40,"tokens_saved":60,"duration_ms":1,"messages_dropped":0,"messages_compacted":2,"summary_chars":20,"active_messages":3}),
            json!({"type":"context_usage","model":"deepseek-flash","used_tokens":42,"window_tokens":1048576,"soft_ratio":0.8,"critical_ratio":0.9,"compacting":false,"estimated":true}),
            json!({"type":"completed"}),
        ] {
            reply.event(&event).unwrap();
        }
        assert_eq!(reply.text, "first\n\nsecond");
        assert_eq!(reply.steps[0]["text"], "inspect data");
        assert_eq!(reply.steps[0]["state"], "completed");
        assert_eq!(reply.tools[0]["state"], "completed");
        assert_eq!(reply.compactions[0]["tokens_saved"], 60);
        assert_eq!(reply.context_usage.as_ref().unwrap()["used_tokens"], 42);
    }

    #[test]
    fn reply_rejects_unknown_unpublished_or_inconsistent_events_and_limits() {
        for event in [
            json!({"type":"unknown"}),
            json!({"type":"tool_start","id":"tool-1","name":"not_published"}),
            json!({"type":"tool_done","id":"tool-1","name":"case_read","failed":false}),
            json!({"type":"compaction","trigger":"manual","pre_tokens":"bad","post_tokens":null,"tokens_saved":null,"duration_ms":null,"messages_dropped":null,"messages_compacted":null,"summary_chars":null,"active_messages":null}),
            json!({"type":"context_usage","model":"unknown","used_tokens":1,"window_tokens":100,"soft_ratio":0.8,"critical_ratio":0.9,"compacting":false,"estimated":true}),
        ] {
            assert!(Reply::default().event(&event).is_err());
        }
        assert!(
            Reply::default()
                .event(&json!({"type":"reasoning_delta","text":"x".repeat(65537)}))
                .is_err()
        );
        let mut reply = Reply::default();
        assert!(
            reply
                .event(&json!({"type":"text_delta","text":"x".repeat(65537)}))
                .is_err()
        );
    }

    #[test]
    fn close_only_finishes_running_reasoning_steps() {
        let mut reply = Reply {
            steps: vec![
                json!({"kind":"reasoning","state":"running"}),
                json!({"kind":"tool","state":"running"}),
                json!({"kind":"reasoning","state":"completed"}),
            ],
            ..Default::default()
        };
        reply.close("failed");
        assert_eq!(reply.steps[0]["state"], "failed");
        assert_eq!(reply.steps[1]["state"], "running");
        assert_eq!(reply.steps[2]["state"], "completed");
    }
}
