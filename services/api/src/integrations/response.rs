use crate::platform::*;
use axum::http::StatusCode;
use serde_json::Value;

pub(super) fn unavailable(code: &'static str, message: &'static str) -> Error {
    Error(StatusCode::SERVICE_UNAVAILABLE, code, message)
}

pub(crate) async fn response_json(mut response: reqwest::Response) -> Result<Value> {
    if !response.status().is_success() {
        return Err(unavailable(
            "upstream_request_failed",
            "Connected service rejected the request; no result was retrieved",
        ));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(anyhow::Error::from)? {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err(unavailable(
                "upstream_result_too_large",
                "Connected service response exceeds the limit",
            ));
        }
        bytes.extend(chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        unavailable(
            "upstream_invalid_response",
            "Connected service returned invalid JSON",
        )
    })
}

pub(super) fn select(value: &Value, fields: &[&str]) -> Value {
    Value::Object(
        fields
            .iter()
            .filter_map(|k| value.get(*k).map(|v| ((*k).to_owned(), v.clone())))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    async fn response(status: u16, body: Vec<u8>) -> reqwest::Response {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = [0u8; 1024];
            let _ = stream.read(&mut request).await;
            stream
                .write_all(
                    format!(
                        "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            stream.write_all(&body).await.unwrap();
        });
        reqwest::get(format!("http://{address}")).await.unwrap()
    }

    #[tokio::test]
    async fn upstream_response_requires_success_bounded_valid_json() {
        assert_eq!(
            response_json(response(200, br#"{"ok":true}"#.to_vec()).await)
                .await
                .unwrap(),
            json!({"ok":true})
        );
        assert_eq!(
            response_json(response(503, b"{}".to_vec()).await)
                .await
                .unwrap_err()
                .1,
            "upstream_request_failed"
        );
        assert_eq!(
            response_json(response(200, b"not-json".to_vec()).await)
                .await
                .unwrap_err()
                .1,
            "upstream_invalid_response"
        );
        assert_eq!(
            response_json(response(200, vec![b'x'; 1024 * 1024 + 1]).await)
                .await
                .unwrap_err()
                .1,
            "upstream_result_too_large"
        );
    }

    #[test]
    fn select_is_an_explicit_allowlist() {
        assert_eq!(
            select(
                &json!({"name":"SO-1","customer":"C1","secret":"hidden"}),
                &["name", "customer"]
            ),
            json!({"name":"SO-1","customer":"C1"})
        );
    }
}
