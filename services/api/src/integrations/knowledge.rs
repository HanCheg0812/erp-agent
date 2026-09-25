use super::{
    response::{response_json, select, unavailable},
    types::Call,
};
use crate::platform::*;
use axum::http::StatusCode;
use serde::Deserialize;
use serde_json::{Value, json};

fn scoped_uri(uri: &str, roots: &[String]) -> bool {
    uri.starts_with("viking://resources/")
        && !uri.contains(['%', '\\', '?', '#'])
        && uri
            .trim_start_matches("viking://")
            .split('/')
            .all(|s| !s.is_empty() && s != "." && s != "..")
        && roots
            .iter()
            .any(|r| uri == r || uri.starts_with(&format!("{}/", r.trim_end_matches('/'))))
}

fn search(arguments: Value) -> Result<(String, u32)> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Search {
        query: String,
        limit: Option<u32>,
    }
    let input: Search = parse(arguments)?;
    text(&input.query, 1000)?;
    let limit = input.limit.unwrap_or(5);
    if !(1..=10).contains(&limit) {
        return Err(Error::bad("Limit must be 1-10"));
    }
    Ok((input.query, limit))
}

fn read_uri(arguments: Value, roots: &[String]) -> Result<String> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Read {
        uri: String,
    }
    let input: Read = parse(arguments)?;
    if input.uri.len() > 1000 || !scoped_uri(&input.uri, roots) {
        return Err(Error::forbidden());
    }
    Ok(input.uri)
}
pub(super) async fn knowledge(app: &App, call: &Call, roots: Vec<String>) -> Result<Value> {
    let origin = std::env::var("OPENVIKING_URL").ok().filter(|s| !s.is_empty()).ok_or_else(|| unavailable("openviking_not_configured", "OpenViking endpoint and embedding service are not configured yet. No knowledge search was performed."))?;
    if roots.is_empty() {
        return Err(Error(
            StatusCode::FORBIDDEN,
            "knowledge_scope_missing",
            "No knowledge resources are granted to this user in the current case",
        ));
    }
    if roots
        .iter()
        .any(|r| !scoped_uri(r, std::slice::from_ref(r)))
    {
        return Err(Error::forbidden());
    }
    let request = if call.tool == "search_case_knowledge" {
        let (query, limit) = search(call.arguments.clone())?;
        app.http.post(format!("{}/api/v1/search/find",origin.trim_end_matches('/'))).timeout(std::time::Duration::from_secs(8)).json(&json!({"query":query,"target_uri":roots,"limit":limit,"context_type":"resource","include_provenance":true}))
    } else {
        let uri = read_uri(call.arguments.clone(), &roots)?;
        app.http
            .get(format!(
                "{}/api/v1/content/read",
                origin.trim_end_matches('/')
            ))
            .timeout(std::time::Duration::from_secs(8))
            .query(&[("uri", uri), ("limit", "160".into())])
    };
    let request = if let Ok(key) = std::env::var("OPENVIKING_API_KEY") {
        request.header("X-API-Key", key)
    } else {
        request
    };
    let value = response_json(request.send().await.map_err(anyhow::Error::from)?).await?;
    if call.tool == "search_case_knowledge" {
        let hits: Vec<_> = value["result"]["resources"]
            .as_array()
            .ok_or_else(|| {
                unavailable(
                    "knowledge_invalid_result",
                    "Knowledge service returned an unexpected search result",
                )
            })?
            .iter()
            .filter(|v| v["uri"].as_str().is_some_and(|uri| scoped_uri(uri, &roots)))
            .take(10)
            .map(|v| {
                select(
                    v,
                    &[
                        "uri",
                        "abstract",
                        "score",
                        "context_type",
                        "level",
                        "provenance",
                    ],
                )
            })
            .collect();
        Ok(json!({"source":"OpenViking","resources":hits}))
    } else {
        Ok(json!({"source":"OpenViking","content":value["result"]}))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_paths_cannot_escape_scope() {
        let roots = vec!["viking://resources/tenant/user/case".into()];
        assert!(scoped_uri(
            "viking://resources/tenant/user/case/document.md",
            &roots
        ));
        for path in [
            "viking://resources/tenant/user/case-other/x",
            "viking://resources/tenant/user/case/../other",
            "viking://resources/tenant/user/case/%2e%2e/other",
            "viking://resources/tenant/other/case/x",
        ] {
            assert!(!scoped_uri(path, &roots));
        }
    }

    #[test]
    fn search_and_read_arguments_are_strict_and_bounded() {
        let roots = vec!["viking://resources/tenant/user/case".into()];
        assert_eq!(
            search(json!({"query":"evidence"})).unwrap(),
            ("evidence".into(), 5)
        );
        assert_eq!(
            search(json!({"query":"evidence","limit":10})).unwrap().1,
            10
        );
        for value in [
            json!({"query":""}),
            json!({"query":"x","limit":0}),
            json!({"query":"x","limit":11}),
            json!({"query":"x","extra":true}),
        ] {
            assert!(search(value).is_err());
        }
        assert_eq!(
            read_uri(
                json!({"uri":"viking://resources/tenant/user/case/doc"}),
                &roots
            )
            .unwrap(),
            "viking://resources/tenant/user/case/doc"
        );
        for value in [
            json!({"uri":"viking://resources/tenant/other/doc"}),
            json!({"uri":"x".repeat(1001)}),
            json!({"uri":"viking://resources/tenant/user/case/doc","extra":true}),
        ] {
            assert!(read_uri(value, &roots).is_err());
        }
    }
}
