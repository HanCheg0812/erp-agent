use crate::platform::*;
use axum::http::StatusCode;
use serde_json::Value;
pub(super) fn configured() -> bool {
    std::env::var("CUBE_API_URL").is_ok() && std::env::var("CUBE_TEMPLATE_ID").is_ok()
}
pub(super) async fn call(
    app: &App,
    path: &str,
    method: reqwest::Method,
    body: Option<Value>,
) -> Result<Value> {
    let url = format!("{}{}", env("CUBE_API_URL", "").trim_end_matches('/'), path);
    let mut request = app.http.request(method.clone(), url);
    if let Ok(key) = std::env::var("CUBE_API_KEY") {
        request = request.header("X-API-Key", key)
    }
    if let Some(body) = body {
        request = request.json(&body)
    }
    let response = request.send().await.map_err(|_| unavailable())?;
    if response.status() == 404
        && (method == reqwest::Method::GET || method == reqwest::Method::DELETE)
    {
        return Ok(Value::Null);
    }
    if !response.status().is_success() {
        return Err(unavailable());
    }
    if response.status() == 204 {
        return Ok(Value::Null);
    }
    response.json().await.map_err(|_| unavailable())
}
pub(super) fn unavailable() -> Error {
    Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "cube_unavailable",
        "Runtime provider is unavailable",
    )
}
pub(super) async fn preflight(app: &App) -> Result<()> {
    let template = env("CUBE_TEMPLATE_ID", "");
    if template.is_empty()
        || !template
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(unavailable());
    }
    let t = call(
        app,
        &format!("/templates/{template}"),
        reqwest::Method::GET,
        None,
    )
    .await?;
    let nodes = call(app, "/cubeapi/v1/nodes", reqwest::Method::GET, None).await?;
    if !template_ready(&t, &nodes) {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_not_ready",
            "Cube template has no ready replica on a healthy node",
        ));
    }
    Ok(())
}

fn template_ready(template: &Value, nodes: &Value) -> bool {
    template["status"] == "READY"
        && template["replicas"].as_array().is_some_and(|replicas| {
            replicas.iter().any(|replica| {
                replica["status"] == "READY"
                    && nodes.as_array().is_some_and(|nodes| {
                        nodes.iter().any(|node| {
                            node["healthy"] == true
                                && node["hostIP"].is_string()
                                && node["hostIP"] == replica["node_ip"]
                        })
                    })
            })
        })
}
pub(super) fn matches(remote: &Value, row: &Value) -> bool {
    let m = &remote["metadata"];
    [
        ("erp_operation", "id"),
        ("erp_case", "case_id"),
        ("erp_tenant", "tenant_id"),
        ("erp_session", "session_id"),
    ]
    .iter()
    .all(|(a, b)| m[*a].is_string() && m[*a] == row[*b])
        && (m.get("erp_user").is_none() || m["erp_user"] == row["actor_id"])
}
pub(super) fn remote_id(remote: &Value) -> Result<&str> {
    remote["sandboxID"]
        .as_str()
        .filter(|s| {
            !s.is_empty()
                && s.len() < 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .ok_or_else(unavailable)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn row() -> Value {
        json!({"id":"operation-1","case_id":"case-1","tenant_id":"tenant-1","session_id":"session-1","actor_id":"user-1"})
    }

    #[test]
    fn provider_metadata_must_match_the_persisted_scope() {
        let remote = json!({"metadata":{"erp_operation":"operation-1","erp_case":"case-1","erp_tenant":"tenant-1","erp_session":"session-1","erp_user":"user-1"}});
        assert!(matches(&remote, &row()));
        for key in [
            "erp_operation",
            "erp_case",
            "erp_tenant",
            "erp_session",
            "erp_user",
        ] {
            let mut changed = remote.clone();
            changed["metadata"][key] = json!("other");
            assert!(!matches(&changed, &row()), "{key}");
        }
        let mut legacy = remote;
        legacy["metadata"]
            .as_object_mut()
            .unwrap()
            .remove("erp_user");
        assert!(matches(&legacy, &row()));
    }

    #[test]
    fn remote_ids_and_ready_replicas_are_strictly_validated() {
        assert_eq!(
            remote_id(&json!({"sandboxID":"cube_ABC-123"})).unwrap(),
            "cube_ABC-123"
        );
        for value in [
            json!(null),
            json!(""),
            json!("bad/id"),
            json!("x".repeat(128)),
        ] {
            assert!(remote_id(&json!({"sandboxID":value})).is_err());
        }
        let template =
            json!({"status":"READY","replicas":[{"status":"READY","node_ip":"10.0.0.1"}]});
        let nodes = json!([{"healthy":true,"hostIP":"10.0.0.1"}]);
        assert!(template_ready(&template, &nodes));
        for changed in [
            json!({"status":"BUILDING","replicas":template["replicas"]}),
            json!({"status":"READY","replicas":[{"status":"FAILED","node_ip":"10.0.0.1"}]}),
            json!({"status":"READY","replicas":[{"status":"READY","node_ip":"10.0.0.2"}]}),
        ] {
            assert!(!template_ready(&changed, &nodes));
        }
        assert!(!template_ready(
            &template,
            &json!([{"healthy":false,"hostIP":"10.0.0.1"}])
        ));
    }
}
