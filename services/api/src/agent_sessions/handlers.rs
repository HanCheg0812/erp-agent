use super::{documents, registration, storage};
use crate::{auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;
use uuid::Uuid;
pub async fn create(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(registration::create(app, actor, id).await?),
    ))
}
pub async fn claim_route(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<String>,
) -> Result<Json<Value>> {
    Ok(Json(registration::claim_route(app, actor, id).await?))
}
pub async fn load(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        storage::load(app, id, crate::auth::bearer(&headers)?).await?,
    ))
}
pub async fn save(
    State(app): State<App>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        storage::save(app, id, crate::auth::bearer(&headers)?, value).await?,
    ))
}
pub async fn document_get(
    State(app): State<App>,
    Path(scope): Path<(String, String)>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        documents::document_get(app, scope, crate::auth::bearer(&headers)?).await?,
    ))
}
pub async fn document_put(
    State(app): State<App>,
    Path(scope): Path<(String, String)>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        documents::document_put(app, scope, crate::auth::bearer(&headers)?, value).await?,
    ))
}
