use crate::{auth::Principal, cases::Page, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde_json::Value;
use uuid::Uuid;

pub async fn history(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<Json<Value>> {
    Ok(Json(
        super::application::history::history(app, actor, id, page).await?,
    ))
}

pub async fn update_settings(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        super::application::settings::update_settings(app, actor, id, value).await?,
    ))
}

pub async fn send(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (created, value) = super::application::messages::send(app, actor, id, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(value),
    ))
}

pub async fn compact(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (created, value) = super::application::messages::compact(app, actor, id, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(value),
    ))
}

pub async fn cancel(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<Json<Value>> {
    Ok(Json(
        super::application::cancellation::cancel(app, actor, id, value).await?,
    ))
}
