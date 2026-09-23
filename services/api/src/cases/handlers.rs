use crate::{
    auth::Principal,
    cases::{application, types::Page},
    platform::*,
};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::Value;
use uuid::Uuid;

pub async fn start_conversation(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (created, record) = application::start_conversation(&app, &actor, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(record),
    ))
}

pub async fn set_context(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        application::set_context(&app, &actor, id, value).await?,
    ))
}

pub async fn list(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    Ok(Json(application::list(&app, &actor, &page).await?))
}

pub async fn create(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    Ok((
        StatusCode::CREATED,
        Json(application::create(&app, &actor, value).await?),
    ))
}

pub async fn detail(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(application::detail_value(&app, &actor, id).await?))
}

pub async fn propose(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let proposal = application::propose(&app, &actor, id, value).await?;
    Ok((StatusCode::CREATED, Json(proposal)))
}

pub async fn approve(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let key = headers
        .get("idempotency-key")
        .and_then(|value| value.to_str().ok());
    let (created, result) = application::approve(&app, &actor, id, key, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(result),
    ))
}

pub async fn events(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    Ok(Json(application::events(&app, &actor, id, &page).await?))
}
