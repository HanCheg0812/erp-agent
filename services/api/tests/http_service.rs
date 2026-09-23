mod support;

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use erp_agent_api::router;
use serde_json::{Value, json};
use tower::ServiceExt;

async fn request(path: &str, body: Body) -> axum::response::Response {
    router(support::app_without_live_dependencies())
        .oneshot(Request::builder().uri(path).body(body).unwrap())
        .await
        .unwrap()
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}

#[tokio::test]
async fn health_is_a_public_library_router_contract() {
    let response = request("/health", Body::empty()).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(
        json_body(response).await,
        json!({"status":"ok","backend":"rust"})
    );
}

#[tokio::test]
async fn protected_service_routes_reject_missing_bearer_before_database_access() {
    let response = request("/v1/cases", Body::empty()).await;
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(response.headers()["cache-control"], "no-store");
    assert_eq!(json_body(response).await["error"], "unauthorized");
}

#[tokio::test]
async fn router_applies_body_limit_and_no_store_to_failures() {
    let size = 8 * 1024 * 1024 + 1;
    let response = router(support::app_without_live_dependencies())
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/auth/login")
                .header("content-type", "application/json")
                .header("content-length", size)
                .body(Body::from(vec![b'x'; size]))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(response.headers()["cache-control"], "no-store");

    let response = request("/does-not-exist", Body::empty()).await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    assert_eq!(response.headers()["cache-control"], "no-store");
}
