use super::{gateway, types::Call};
use crate::platform::*;
use axum::{
    Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;

pub async fn call(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Call>,
) -> Result<Json<Value>> {
    Ok(Json(
        gateway::call(app, id, crate::auth::bearer(&headers)?, input).await?,
    ))
}
