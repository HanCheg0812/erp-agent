use super::provider;
use crate::{agent_sessions, platform::*};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};
use tokio::net::TcpStream;
use tokio_tungstenite::{MaybeTlsStream, WebSocketStream, connect_async, tungstenite::Message};
use url::Url;
use uuid::Uuid;

const TOOL: &str = "cube_browser_browse";
const MAX_EXCERPT: usize = 4_000;
type Socket = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrowserInput {
    tool_call_id: String,
    url: String,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct PageSummary {
    title: String,
    heading: String,
    excerpt: String,
}

fn template_id() -> Result<String> {
    let id = std::env::var("CUBE_BROWSER_TEMPLATE_ID").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_browser_not_configured",
            "Browser tool template is not configured",
        )
    })?;
    if id.is_empty()
        || id.len() >= 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::bad("Invalid browser tool template"));
    }
    Ok(id)
}

fn allowed_hosts() -> Result<Vec<String>> {
    let raw = std::env::var("CUBE_BROWSER_ALLOWED_HOSTS").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_browser_not_configured",
            "Browser tool host allowlist is not configured",
        )
    })?;
    let hosts = raw
        .split(',')
        .map(|host| host.trim().to_ascii_lowercase())
        .filter(|host| !host.is_empty())
        .collect::<Vec<_>>();
    if hosts.is_empty()
        || hosts.iter().any(|host| {
            host.contains('*')
                || host.contains('/')
                || host.contains(':')
                || host.parse::<std::net::IpAddr>().is_ok()
                || !host
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
        })
    {
        return Err(Error::bad("Invalid browser tool host allowlist"));
    }
    Ok(hosts)
}

fn validate(input: &BrowserInput) -> Result<Url> {
    let hosts = allowed_hosts()?;
    validate_with_hosts(input, &hosts)
}

fn validate_with_hosts(input: &BrowserInput, hosts: &[String]) -> Result<Url> {
    if input.tool_call_id.is_empty()
        || input.tool_call_id.len() > 256
        || !input
            .tool_call_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || input.url.len() > 2_048
    {
        return Err(Error::bad("Invalid browser input"));
    }
    let url = Url::parse(&input.url).map_err(|_| Error::bad("Invalid browser URL"))?;
    let host = url
        .host_str()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| Error::bad("Browser URL must include a host"))?;
    if url.scheme() != "https"
        || url.username() != ""
        || url.password().is_some()
        || url.port().is_some_and(|port| port != 443)
        || host == "localhost"
        || host.ends_with(".localhost")
        || host.parse::<std::net::IpAddr>().is_ok()
        || !hosts.iter().any(|allowed| allowed == &host)
    {
        return Err(Error::forbidden());
    }
    Ok(url)
}

async fn find_created(app: &App, operation: Uuid, template: &str) -> Result<Option<String>> {
    let inventory = provider::call(
        app,
        &format!("/sandboxes?metadata=erp_tool_operation%3D{operation}&limit=1000"),
        reqwest::Method::GET,
        None,
    )
    .await?;
    let matches: Vec<&Value> = inventory
        .as_array()
        .ok_or_else(provider::unavailable)?
        .iter()
        .filter(|item| {
            item["metadata"]["erp_tool_operation"] == operation.to_string()
                && item["metadata"]["project"] == "erp-agent"
                && item["templateID"] == template
        })
        .collect();
    if matches.len() > 1 {
        return Err(Error::conflict("cube_browser_duplicate_instance"));
    }
    matches
        .first()
        .map(|item| provider::remote_id(item).map(str::to_owned))
        .transpose()
}

fn matches_scope(remote: &Value, metadata: &Value, template: &str) -> bool {
    remote["templateID"] == template
        && metadata.as_object().is_some_and(|fields| {
            fields
                .iter()
                .all(|(key, value)| remote["metadata"].get(key) == Some(value))
        })
}

async fn release(app: &App, id: &str) -> bool {
    if provider::call(
        app,
        &format!("/sandboxes/{id}"),
        reqwest::Method::DELETE,
        None,
    )
    .await
    .is_err()
    {
        return false;
    }
    matches!(
        provider::call(app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await,
        Ok(Value::Null)
    )
}

fn browser_origin(id: &str) -> Result<String> {
    let domain = env("CUBE_BROWSER_DOMAIN", "cube.app");
    if domain.is_empty()
        || domain.len() >= 256
        || !domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
    {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_browser_not_configured",
            "CUBE_BROWSER_DOMAIN is invalid",
        ));
    }
    let proxy = std::env::var("CUBE_PROXY_URL").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_browser_not_configured",
            "Cube proxy is not configured",
        )
    })?;
    let scheme = Url::parse(&proxy)
        .map_err(|_| provider::unavailable())?
        .scheme()
        .to_owned();
    let scheme = if scheme == "https" { "https" } else { "http" };
    Ok(format!("{scheme}://9000-{id}.{domain}"))
}

fn truncate(value: &str, limit: usize) -> String {
    let value = value.trim();
    if value.len() <= limit {
        value.to_owned()
    } else {
        if limit < '…'.len_utf8() {
            return String::new();
        }
        let mut end = limit - '…'.len_utf8();
        while !value.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &value[..end])
    }
}

async fn cdp_command(
    socket: &mut Socket,
    next_id: &mut u64,
    session_id: Option<&str>,
    method: &str,
    params: Value,
) -> Result<Value> {
    let id = *next_id;
    *next_id += 1;
    let mut request = json!({"id":id,"method":method,"params":params});
    if let Some(session_id) = session_id {
        request["sessionId"] = json!(session_id);
    }
    socket
        .send(Message::text(request.to_string()))
        .await
        .map_err(|_| provider::unavailable())?;
    let response = tokio::time::timeout(Duration::from_secs(12), async {
        while let Some(message) = socket.next().await {
            let message = message.map_err(|_| provider::unavailable())?;
            let Message::Text(text) = message else {
                continue;
            };
            let value: Value = serde_json::from_str(&text).map_err(|_| provider::unavailable())?;
            if value["id"] != id {
                continue;
            }
            if value["error"].is_object() {
                return Err(provider::unavailable());
            }
            return Ok(value["result"].clone());
        }
        Err(provider::unavailable())
    })
    .await
    .map_err(|_| provider::unavailable())??;
    Ok(response)
}

async fn cdp_version(app: &App, origin: &str) -> Result<Value> {
    // Cube reports the sandbox as created before Chromium has opened its CDP
    // listener. Treat the short 502/connection-refused window as startup
    // rather than making a healthy browser template look unavailable.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut delay = Duration::from_millis(200);
    loop {
        let response = app
            .http
            .get(format!("{origin}/cdp/json/version"))
            .timeout(Duration::from_secs(4))
            .send()
            .await;
        if let Ok(response) = response {
            if response.status().is_success() {
                if let Ok(version) = response.json::<Value>().await {
                    return Ok(version);
                }
            }
        }
        if Instant::now() >= deadline {
            return Err(provider::unavailable());
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(Duration::from_secs(1));
    }
}

async fn read_page(app: &App, sandbox_id: &str, url: &Url) -> Result<PageSummary> {
    let origin = browser_origin(sandbox_id)?;
    let version = cdp_version(app, &origin).await?;
    let ws_url = version["webSocketDebuggerUrl"]
        .as_str()
        .filter(|value| value.starts_with("ws://") || value.starts_with("wss://"))
        .ok_or_else(provider::unavailable)?;
    let (mut socket, _) = connect_async(ws_url)
        .await
        .map_err(|_| provider::unavailable())?;
    let mut next_id = 1;
    let target = cdp_command(
        &mut socket,
        &mut next_id,
        None,
        "Target.createTarget",
        json!({"url":"about:blank"}),
    )
    .await?;
    let target_id = target["targetId"]
        .as_str()
        .ok_or_else(provider::unavailable)?;
    let attached = cdp_command(
        &mut socket,
        &mut next_id,
        None,
        "Target.attachToTarget",
        json!({"targetId":target_id,"flatten":true}),
    )
    .await?;
    let session_id = attached["sessionId"]
        .as_str()
        .ok_or_else(provider::unavailable)?;
    cdp_command(
        &mut socket,
        &mut next_id,
        Some(session_id),
        "Page.enable",
        json!({}),
    )
    .await?;
    cdp_command(
        &mut socket,
        &mut next_id,
        Some(session_id),
        "Runtime.enable",
        json!({}),
    )
    .await?;
    let mut summary = None;
    for attempt in 0..4 {
        let navigation = cdp_command(
            &mut socket,
            &mut next_id,
            Some(session_id),
            "Page.navigate",
            json!({"url":url.as_str()}),
        )
        .await?;
        tokio::time::sleep(Duration::from_millis(700)).await;
        let evaluated = cdp_command(
            &mut socket,
            &mut next_id,
            Some(session_id),
            "Runtime.evaluate",
            json!({
                "expression":"JSON.stringify({url:location.href,title:document.title || '',heading:document.querySelector('h1')?.innerText || '',text:document.body?.innerText || ''})",
                "returnByValue":true
            }),
        )
        .await?;
        let payload = evaluated["result"]["value"]
            .as_str()
            .ok_or_else(provider::unavailable)?;
        let payload: Value = serde_json::from_str(payload).map_err(|_| provider::unavailable())?;
        let final_url = payload["url"].as_str().unwrap_or_default();
        let same_host = Url::parse(final_url).ok().is_some_and(|final_url| {
            final_url.scheme() == "https" && final_url.host_str() == url.host_str()
        });
        if navigation["errorText"].is_null() && same_host {
            summary = Some(PageSummary {
                title: truncate(payload["title"].as_str().unwrap_or_default(), 500),
                heading: truncate(payload["heading"].as_str().unwrap_or_default(), 500),
                excerpt: truncate(payload["text"].as_str().unwrap_or_default(), MAX_EXCERPT),
            });
            break;
        }
        if attempt < 3 {
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }
    let _ = cdp_command(
        &mut socket,
        &mut next_id,
        None,
        "Target.closeTarget",
        json!({"targetId":target_id}),
    )
    .await;
    summary.ok_or(Error(
        StatusCode::BAD_GATEWAY,
        "cube_browser_navigation_failed",
        "Browser navigation failed",
    ))
}

async fn record_result(
    app: &App,
    operation: Uuid,
    sandbox_id: Option<&str>,
    result: Option<&Value>,
    error_code: Option<&str>,
    cleanup_confirmed: bool,
) -> Result<()> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let state = if result.is_some() && cleanup_confirmed {
        "completed"
    } else {
        "failed"
    };
    let changed = tx
        .execute(
            "UPDATE cube_browser_tool_calls SET sandbox_id=COALESCE($2,sandbox_id),state=$3,result=$4,error_code=$5,cleanup_confirmed=$6,updated_at=now() WHERE id=$1 AND state<>'completed'",
            &[&operation, &sandbox_id, &state, &result, &error_code, &cleanup_confirmed],
        )
        .await?;
    if state == "completed" && changed == 1 {
        tx.execute(
            "INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) SELECT c.tenant_id,c.case_id,s.owner_id,'agent.cube_browser_completed',jsonb_build_object('operation_id',c.id,'tool_call_id',c.tool_call_id,'template_id',c.template_id,'sandbox_id',c.sandbox_id,'url',c.url,'cleanup_confirmed',true) FROM cube_browser_tool_calls c JOIN agent_sessions s ON s.id=c.session_id WHERE c.id=$1",
            &[&operation],
        )
        .await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn browse(
    State(app): State<App>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(input): Json<BrowserInput>,
) -> Result<Json<Value>> {
    let url = validate(&input)?;
    let template = template_id()?;
    let token = crate::auth::bearer(&headers)?;
    let operation = Uuid::new_v4();
    let digest = json_digest(&json!(input.url));
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &session, &token, false).await?;
    let scope = tx
        .query_opt(
            "SELECT s.tenant_id,s.owner_id,s.case_id,r.message_id FROM agent_sessions s JOIN case_conversations c ON c.session_id=s.id AND c.tenant_id=s.tenant_id AND c.case_id=s.case_id JOIN agent_runs r ON r.case_id=c.case_id AND r.tenant_id=c.tenant_id WHERE s.id=$1 AND r.state='running' AND NOT r.cancel_requested",
            &[&session],
        )
        .await?
        .ok_or_else(Error::forbidden)?;
    let tenant: Uuid = scope.get(0);
    let actor: Uuid = scope.get(1);
    let case: Uuid = scope.get(2);
    let message: i64 = scope.get(3);
    let inserted = tx
        .query_opt(
            "INSERT INTO cube_browser_tool_calls(id,tenant_id,case_id,session_id,message_id,tool_call_id,url,input_digest,template_id,state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'creating') ON CONFLICT(session_id,tool_call_id) DO NOTHING RETURNING id",
            &[&operation, &tenant, &case, &session, &message, &input.tool_call_id, &input.url, &digest, &template],
        )
        .await?
        .is_some();
    if !inserted {
        let row = tx
            .query_one(
                "SELECT message_id,input_digest,template_id,state,result FROM cube_browser_tool_calls WHERE session_id=$1 AND tool_call_id=$2",
                &[&session, &input.tool_call_id],
            )
            .await?;
        if row.get::<_, i64>(0) != message
            || row.get::<_, String>(1) != digest
            || row.get::<_, String>(2) != template
        {
            return Err(Error::conflict("cube_tool_call_mismatch"));
        }
        let state: String = row.get(3);
        let result: Option<Value> = row.get(4);
        tx.commit().await?;
        return match (state.as_str(), result) {
            ("completed", Some(value)) => Ok(Json(value)),
            ("failed", _) => Err(Error::conflict("cube_tool_failed")),
            _ => Err(Error::conflict("cube_tool_in_progress")),
        };
    }
    tx.commit().await?;
    drop(db);

    let mut sandbox_id = None;
    let mut creation_attempted = false;
    let mut page = None;
    let outcome = async {
        provider::preflight(&app, &template).await?;
        let metadata = json!({"project":"erp-agent","erp_tool_operation":operation,
            "erp_tenant":tenant,"erp_case":case,"erp_session":session,"erp_message":message.to_string(),"erp_user":actor,"erp_tool":TOOL});
        let request = json!({"templateID":template,"timeout":120,"allow_internet_access":false,
            "network":{"allow_out":[url.host_str().expect("validated browser URL has a host")]},
            "metadata":metadata});
        creation_attempted = true;
        let created = match provider::call(&app, "/sandboxes", reqwest::Method::POST, Some(request)).await {
            Ok(value) => value,
            Err(error) => match find_created(&app, operation, &template).await? {
                Some(id) => provider::call(&app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await?,
                None => return Err(error),
            },
        };
        let id = provider::remote_id(&created)?.to_owned();
        let remote = provider::call(&app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await?;
        if !matches_scope(&remote, &metadata, &template) {
            return Err(Error::conflict("cube_browser_scope_mismatch"));
        }
        sandbox_id = Some(id.clone());
        let db = app.db.get().await?;
        db.execute("UPDATE cube_browser_tool_calls SET sandbox_id=$2,state='running',updated_at=now() WHERE id=$1", &[&operation,&id]).await?;
        drop(db);
        page = Some(read_page(&app, &id, &url).await?);
        Ok(())
    }
    .await;
    if sandbox_id.is_none() {
        match find_created(&app, operation, &template).await {
            Ok(found) => sandbox_id = found,
            Err(_) => {
                record_result(
                    &app,
                    operation,
                    None,
                    None,
                    Some("cube_tool_cleanup_unconfirmed"),
                    false,
                )
                .await?;
                return Err(Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "cube_tool_cleanup_unconfirmed",
                    "Browser tool cleanup could not be confirmed",
                ));
            }
        }
    }
    let cleaned = match sandbox_id.as_deref() {
        Some(id) => release(&app, id).await,
        None => !creation_attempted,
    };
    let result = page.map(|page| {
        json!({"tool":TOOL,"template_id":template,"sandbox_id":sandbox_id,
        "url":url.as_str(),"title":page.title,"heading":page.heading,"excerpt":page.excerpt,
        "cleanup_confirmed":cleaned})
    });
    let error_code = if !cleaned && sandbox_id.is_none() {
        Some("cube_tool_cleanup_unconfirmed")
    } else if !cleaned {
        Some("cube_tool_cleanup_failed")
    } else {
        outcome.as_ref().err().map(|error| error.1)
    };
    record_result(
        &app,
        operation,
        sandbox_id.as_deref(),
        if error_code.is_none() {
            result.as_ref()
        } else {
            None
        },
        error_code,
        cleaned,
    )
    .await?;
    if let Some(code) = error_code {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            code,
            "Browser tool execution failed",
        ));
    }
    Ok(Json(result.expect("successful browser result")))
}

pub(super) async fn sweep_stale(app: &App) {
    let Ok(db) = app.db.get().await else { return };
    let Ok(rows) = db
        .query("SELECT id,template_id,sandbox_id FROM cube_browser_tool_calls WHERE created_at<now()-interval '90 seconds' AND (state IN ('creating','running') OR NOT cleanup_confirmed) ORDER BY updated_at LIMIT 20", &[])
        .await
    else { return };
    drop(db);
    for row in rows {
        let operation: Uuid = row.get(0);
        let template: String = row.get(1);
        let id: Option<String> = row.get(2);
        let id = match id {
            Some(id) => Some(id),
            None => match find_created(app, operation, &template).await {
                Ok(found) => found,
                Err(_) => continue,
            },
        };
        let cleaned = match id.as_deref() {
            Some(id) => release(app, id).await,
            None => false,
        };
        if cleaned {
            let _ = record_result(
                app,
                operation,
                id.as_deref(),
                None,
                Some("cube_tool_interrupted"),
                true,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_excerpt_truncation_preserves_utf8_and_byte_limit() {
        let text = "A中文é🙂页面内容";
        for limit in 0..text.len() {
            let result = truncate(text, limit);
            assert!(result.len() <= limit);
            if limit >= '…'.len_utf8() {
                assert!(text.starts_with(result.strip_suffix('…').unwrap()));
            }
        }
        assert_eq!(truncate(text, text.len()), text);
    }

    #[test]
    fn browser_contract_rejects_non_https_and_credentials() {
        let mut input = BrowserInput {
            tool_call_id: "call_1".into(),
            url: "https://example.com/docs".into(),
        };
        let hosts = vec!["example.com".to_owned()];
        assert_eq!(
            validate_with_hosts(&input, &hosts).unwrap().host_str(),
            Some("example.com")
        );
        for bad in [
            "http://example.com",
            "https://user:pass@example.com",
            "https://127.0.0.1",
        ] {
            input.url = bad.into();
            assert!(validate_with_hosts(&input, &hosts).is_err());
        }
        input.url = "https://other.example.com".into();
        assert!(validate_with_hosts(&input, &hosts).is_err());
    }
}
