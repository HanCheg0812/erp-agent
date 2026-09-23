use super::lifecycle::operate;
use crate::{auth::Principal, platform::*};
use axum::{
    Extension, Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::Value;
use uuid::Uuid;
pub async fn status(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>> {
    Ok(Json(operate(&app, &actor, id, "status", None).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Create {
    request_id: Uuid,
}
pub async fn create(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path(id): Path<Uuid>,
    Json(v): Json<Value>,
) -> Result<Json<Value>> {
    let input: Create = parse(v)?;
    Ok(Json(
        operate(&app, &actor, id, "create", Some(input.request_id)).await?,
    ))
}
pub async fn release(
    State(app): State<App>,
    Extension(actor): Extension<Principal>,
    Path((id, operation)): Path<(Uuid, Uuid)>,
) -> Result<Json<Value>> {
    Ok(Json(
        operate(&app, &actor, id, "release", Some(operation)).await?,
    ))
}
