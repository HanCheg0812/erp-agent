use super::{Principal, accounts, sessions};
use crate::platform::*;
use axum::{
    Extension, Json,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::Response,
};
use serde_json::Value;

pub fn bearer(headers: &HeaderMap) -> Result<String> {
    headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .filter(|v| {
            v.len() == 43
                && v.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        })
        .map(str::to_owned)
        .ok_or_else(Error::unauthorized)
}

pub async fn authenticate(
    State(app): State<App>,
    mut req: Request,
    next: Next,
) -> Result<Response> {
    let token = bearer(req.headers())?;
    req.extensions_mut()
        .insert(sessions::resolve(&app, &token).await?);
    Ok(next.run(req).await)
}
pub async fn register(
    State(app): State<App>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(accounts::register(&app, value).await?),
    ))
}
pub async fn login(State(app): State<App>, Json(value): Json<Value>) -> Result<Json<Value>> {
    Ok(Json(accounts::login(&app, value).await?))
}
pub async fn me(Extension(actor): Extension<Principal>) -> Json<Principal> {
    Json(actor)
}
pub async fn logout(State(app): State<App>, headers: HeaderMap) -> Result<StatusCode> {
    sessions::revoke(&app, &bearer(&headers)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bearer_accepts_only_the_storage_token_alphabet_and_exact_length() {
        let valid = "a".repeat(43);
        let mut headers = HeaderMap::new();
        headers.insert("authorization", format!("Bearer {valid}").parse().unwrap());
        assert_eq!(bearer(&headers).unwrap(), valid);
        for value in [
            "Basic abc".to_owned(),
            "Bearer short".to_owned(),
            format!("Bearer {}", "a".repeat(42)),
            format!("Bearer {}!", "a".repeat(42)),
        ] {
            headers.insert("authorization", value.parse().unwrap());
            assert!(bearer(&headers).is_err());
        }
    }
}
