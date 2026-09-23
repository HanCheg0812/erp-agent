use super::super::{repository, types::Page};
use crate::{auth::Principal, platform::*};
use serde_json::{Value, json};
use uuid::Uuid;

pub(crate) async fn list(app: &App, actor: &Principal, page: &Page) -> Result<Value> {
    let db = app.db.get().await?;
    Ok(json!({"items":repository::list(&**db, actor, page.limit()?).await?}))
}

pub(crate) async fn detail_value(app: &App, actor: &Principal, id: Uuid) -> Result<Value> {
    let mut db = app.db.get().await?;
    let tx = db
        .build_transaction()
        .isolation_level(tokio_postgres::IsolationLevel::RepeatableRead)
        .read_only(true)
        .start()
        .await?;
    let record = repository::detail(&*tx, actor, id).await?;
    tx.commit().await?;
    Ok(record)
}

pub(crate) async fn events(app: &App, actor: &Principal, id: Uuid, page: &Page) -> Result<Value> {
    let after = page
        .after
        .as_deref()
        .unwrap_or("0")
        .parse::<i64>()
        .map_err(|_| Error::bad("Invalid cursor"))?;
    if after < 0 {
        return Err(Error::bad("Invalid cursor"));
    }
    let db = app.db.get().await?;
    repository::find(&**db, actor, id, false).await?;
    let items = repository::events(&**db, actor, id, after, page.limit()?).await?;
    Ok(
        json!({"next_cursor":items.last().map(|item|item["id"].clone()).unwrap_or(json!(after.to_string())),"items":items}),
    )
}
