use super::service;
use crate::{auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::Value;
use uuid::Uuid;

pub async fn overview(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
) -> Result<Json<Value>> {
    Ok(Json(service::overview(&app, &actor).await?))
}

pub async fn create_candidate(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Json(input): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(service::create_candidate(&app, &actor, input).await?),
    ))
}

pub async fn candidate(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(service::candidate(&app, &actor, id).await?))
}

pub async fn build_candidate(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::ACCEPTED,
        Json(service::build_candidate(&app, &actor, id).await?),
    ))
}

pub async fn create_canary(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(service::create_canary(&app, &actor, id).await?),
    ))
}

pub async fn activate_candidate(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(input): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        service::activate_candidate(&app, &actor, id, input).await?,
    ))
}

pub async fn adopt_template(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<String>,
    Json(input): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(service::adopt_template(&app, &actor, &id, input).await?),
    ))
}
