use super::{
    provider::{call, configured, matches, preflight, remote_id},
    reconciliation::reconcile,
    repository::{ROW, save},
};
use crate::{agent_sessions, cases};
use crate::{auth::Principal, platform::*};
use axum::http::StatusCode;
use serde_json::{Value, json};
use uuid::Uuid;
fn view(mut row: Value) -> Value {
    if let Some(o) = row.as_object_mut() {
        for key in ["tenant_id", "case_id", "actor_id", "expired"] {
            o.remove(key);
        }
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_sandbox_view_removes_internal_scope_and_expiry_fields() {
        let value = view(json!({
            "id":"operation","tenant_id":"tenant","case_id":"case","actor_id":"user",
            "expired":false,"state":"provisioned","sandbox_id":"cube-1"
        }));
        for key in ["tenant_id", "case_id", "actor_id", "expired"] {
            assert!(value.get(key).is_none(), "{key}");
        }
        assert_eq!(value["state"], "provisioned");
        assert_eq!(value["sandbox_id"], "cube-1");
    }
}
pub(super) async fn operate(
    app: &App,
    actor: &Principal,
    id: Uuid,
    action: &str,
    operation: Option<Uuid>,
) -> Result<Value> {
    let mut db = app.db.get().await?;
    cases::case(&**db, actor, id, false).await?;
    let key = format!("erp-cube:{}:{id}", actor.tenant_id);
    let locked: bool = db
        .query_one(
            "SELECT pg_try_advisory_lock(hashtextextended($1,0))",
            &[&key],
        )
        .await?
        .get(0);
    if !locked {
        return Err(Error::conflict("sandbox_busy"));
    }
    let result = operate_locked(app, &mut db, actor, id, action, operation).await;
    if db
        .query_one("SELECT pg_advisory_unlock(hashtextextended($1,0))", &[&key])
        .await
        .is_err()
    {
        drop(deadpool_postgres::Object::take(db));
    }
    result
}
async fn operate_locked(
    app: &App,
    db: &mut deadpool_postgres::Object,
    actor: &Principal,
    id: Uuid,
    action: &str,
    operation: Option<Uuid>,
) -> Result<Value> {
    if action != "status" {
        actor.require("planner")?;
    }
    let current=db.query_opt(&format!("SELECT {ROW} FROM case_sandboxes s WHERE tenant_id=$1 AND case_id=$2 ORDER BY created_at DESC,id DESC LIMIT 1"),&[&actor.tenant_id,&id]).await?.map(|r|r.get::<_,Value>(0));
    let row = if action == "create" {
        if !configured() {
            return Err(Error(
                StatusCode::SERVICE_UNAVAILABLE,
                "cube_not_configured",
                "Runtime provider is not configured",
            ));
        }
        let operation = operation.unwrap();
        if let Some(prior) = db
            .query_opt(
                &format!("SELECT {ROW} FROM case_sandboxes s WHERE id=$1"),
                &[&operation],
            )
            .await?
        {
            let prior: Value = prior.get(0);
            if prior["tenant_id"] != actor.tenant_id.to_string()
                || prior["case_id"] != id.to_string()
                || prior["actor_id"] != actor.user_id.to_string()
            {
                return Err(Error::conflict("idempotency_conflict"));
            }
            Some(reconcile(app, db, actor, prior).await?)
        } else {
            if let Some(current) = current {
                if reconcile(app, db, actor, current).await?["state"] != "released" {
                    return Err(Error::conflict("sandbox_exists"));
                }
            }
            preflight(app).await?;
            let tx = db.transaction().await?;
            cases::case(&*tx, actor, id, true).await?;
            let session = agent_sessions::bind(&tx, actor, id).await?;
            let mut row=object(&*tx,&format!("INSERT INTO case_sandboxes AS s(id,tenant_id,case_id,session_id,actor_id,state) VALUES($1,$2,$3,$4,$5,'creating') RETURNING {ROW}"),&[&operation,&actor.tenant_id,&id,&session,&actor.user_id]).await?;
            cases::audit(
                &*tx,
                actor,
                id,
                "sandbox.creating",
                json!({"operation_id":operation,"session_id":session}),
            )
            .await?;
            tx.commit().await?;
            let metadata = json!({"project":"erp-agent","erp_operation":operation,"erp_case":id,"erp_tenant":actor.tenant_id,"erp_user":actor.user_id,"erp_session":session});
            if let Ok(remote)=call(app,"/sandboxes",reqwest::Method::POST,Some(json!({"templateID":env("CUBE_TEMPLATE_ID",""),"timeout":900,"allow_internet_access":false,"metadata":metadata}))).await{
                if matches(&remote,&row){if let Ok(id)=remote_id(&remote){row=save(db,actor,&row,json!({"sandbox_id":id})).await?}}
            }
            Some(reconcile(app, db, actor, row).await?)
        }
    } else if action == "release" {
        let mut row=object(&***db,&format!("SELECT {ROW} FROM case_sandboxes s WHERE id=$1 AND tenant_id=$2 AND case_id=$3 AND actor_id=$4"),&[&operation.unwrap(),&actor.tenant_id,&id,&actor.user_id]).await?;
        if row["state"] != "released" {
            row = save(
                db,
                actor,
                &row,
                json!({"state":"releasing","guest_ready":false}),
            )
            .await?
        }
        Some(reconcile(app, db, actor, row).await?)
    } else {
        match current {
            Some(r) => Some(reconcile(app, db, actor, r).await?),
            None => None,
        }
    };
    Ok(
        json!({"configured":configured(),"can_manage":actor.roles.iter().any(|r|r=="planner"),"operation":row.map(view)}),
    )
}
