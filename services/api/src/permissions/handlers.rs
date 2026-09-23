use super::application::{actions, attachments, decisions, session};
use super::types::Action;
use super::types::Decision;
use crate::{auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
    http::HeaderMap,
};
use serde_json::Value;
use uuid::Uuid;
pub async fn read_case(
    State(app): State<App>,
    Path(session): Path<String>,
    headers: HeaderMap,
) -> Result<Json<Value>> {
    Ok(Json(
        session::read_case(app, session, crate::auth::bearer(&headers)?).await?,
    ))
}
pub async fn read_attachment(
    State(app): State<App>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(input): Json<attachments::ReadAttachment>,
) -> Result<Json<Value>> {
    Ok(Json(
        attachments::read(app, session, crate::auth::bearer(&headers)?, input).await?,
    ))
}
pub async fn call(
    State(app): State<App>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(input): Json<Action>,
) -> Result<Json<Value>> {
    Ok(Json(
        actions::call(app, session, crate::auth::bearer(&headers)?, input).await?,
    ))
}
pub async fn decide(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(ids): Path<(Uuid, Uuid)>,
    Json(input): Json<Decision>,
) -> Result<Json<Value>> {
    Ok(Json(decisions::decide(app, actor, ids, input).await?))
}
