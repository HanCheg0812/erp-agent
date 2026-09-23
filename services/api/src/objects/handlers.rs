use super::application::{documents, memory};
use crate::{auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{IntoResponse, Response},
};
use serde_json::Value;
use uuid::Uuid;

pub async fn list(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(documents::list(app, actor, id).await?))
}
pub async fn upload(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (created, value) = documents::upload(app, actor, id, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(value),
    ))
}
pub async fn download(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Response> {
    let file = documents::download(app, actor, id).await?;
    Ok((
        [
            (header::CONTENT_TYPE, file.content_type.as_str()),
            (header::CONTENT_DISPOSITION, "attachment"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        file.bytes,
    )
        .into_response())
}
pub async fn memory_list(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
) -> Result<Json<Value>> {
    Ok(Json(memory::memory_list(app, actor).await?))
}
pub async fn memory_save(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Json(value): Json<Value>,
) -> Result<(StatusCode, Json<Value>)> {
    let (created, value) = memory::memory_save(app, actor, value).await?;
    Ok((
        if created {
            StatusCode::CREATED
        } else {
            StatusCode::OK
        },
        Json(value),
    ))
}
pub async fn memory_delete(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode> {
    memory::memory_delete(app, actor, id).await?;
    Ok(StatusCode::NO_CONTENT)
}
