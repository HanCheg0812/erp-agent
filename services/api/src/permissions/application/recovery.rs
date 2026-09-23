use super::super::repository::{decisions, recovery as repository};
use super::super::reviewer::{Review, review};
use crate::platform::*;
use serde_json::json;
// The worker holds the case advisory lock; a pending request occupies no tool
// connection. Every continuation rechecks identity and the original arguments.
pub async fn resolve_waiting(app: &App, mid: i64) -> Result<()> {
    let db = app.db.get().await?;
    let rows = repository::pending(&**db, mid).await?;
    drop(db);
    for p in rows {
        let id = uuid(p["id"].as_str().unwrap())?;
        let db = app.db.get().await?;
        let expired = decisions::expired(&**db, id).await?;
        drop(db);
        if !expired && p["mode"] != "auto" {
            continue;
        }
        let result = if expired {
            Review {
                decision: "denied".into(),
                reason: "批准请求已过期".into(),
            }
        } else {
            let db = app.db.get().await?;
            let case_id = uuid(p["case_id"].as_str().unwrap())?;
            let case = repository::case(
                &**db,
                case_id,
                uuid(p["tenant_id"].as_str().unwrap())?,
                uuid(p["owner_id"].as_str().unwrap())?,
            )
            .await?;
            let task = repository::recent_messages(&**db, case_id, mid).await?;
            drop(db);
            review(app,&json!({"user_task":p["user_task"],"recent_user_messages":task,"case":case,"tool":p["tool"],"arguments":p["arguments"],"scope":"Current account and case only. Application record changes only, no business signature, ERP write, or permission change."})).await.unwrap_or(Review { decision:"denied".into(),reason:"自动审核未完成，本次操作未获批准".into() })
        };
        let mut db = app.db.get().await?;
        let tx = db.transaction().await?;
        let active = repository::lock_waiting(&tx, mid).await?;
        if active {
            let changed = repository::record_review(
                &tx,
                id,
                &result.decision,
                if expired { "expiry" } else { "reviewer" },
                &result.reason,
            )
            .await?;
            if changed == 1 {
                repository::audit_review(&tx, id).await?;
            }
        }
        tx.commit().await?;
    }
    let db = app.db.get().await?;
    repository::resume_if_resolved(&**db, mid).await?;
    Ok(())
}
