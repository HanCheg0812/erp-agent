use super::{bridge, provider};
use crate::{agent_sessions, platform::*};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

const TOOL: &str = "cube_quantity_summary";
const MAX_OUTPUT: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuantityInput {
    tool_call_id: String,
    quantities: Vec<i64>,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Summary {
    count: usize,
    total: i64,
    minimum: i64,
    maximum: i64,
}

fn validate(input: &QuantityInput) -> Result<Summary> {
    if input.tool_call_id.is_empty()
        || input.tool_call_id.len() > 256
        || !input
            .tool_call_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        || !(1..=32).contains(&input.quantities.len())
        || input
            .quantities
            .iter()
            .any(|n| !(0..=1_000_000).contains(n))
    {
        return Err(Error::bad("Invalid quantity summary input"));
    }
    Ok(Summary {
        count: input.quantities.len(),
        total: input.quantities.iter().sum(),
        minimum: *input.quantities.iter().min().unwrap(),
        maximum: *input.quantities.iter().max().unwrap(),
    })
}

fn template_id() -> Result<String> {
    let id = std::env::var("CUBE_QUANTITY_TEMPLATE_ID").map_err(|_| {
        Error(
            StatusCode::SERVICE_UNAVAILABLE,
            "cube_tool_not_configured",
            "Quantity tool template is not configured",
        )
    })?;
    if id.is_empty()
        || id.len() >= 128
        || !id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::bad("Invalid quantity tool template"));
    }
    Ok(id)
}

async fn find_created(app: &App, operation: Uuid, template: &str) -> Result<Option<String>> {
    let inventory = provider::call(
        app,
        &format!("/sandboxes?metadata=erp_tool_operation%3D{operation}&limit=1000"),
        reqwest::Method::GET,
        None,
    )
    .await?;
    let matches: Vec<&Value> = inventory
        .as_array()
        .ok_or_else(provider::unavailable)?
        .iter()
        .filter(|item| {
            item["metadata"]["erp_tool_operation"] == operation.to_string()
                && item["metadata"]["project"] == "erp-agent"
                && item["templateID"] == template
        })
        .collect();
    if matches.len() > 1 {
        return Err(Error::conflict("cube_tool_duplicate_instance"));
    }
    matches
        .first()
        .map(|item| provider::remote_id(item).map(str::to_owned))
        .transpose()
}

fn matches_scope(remote: &Value, metadata: &Value, template: &str) -> bool {
    remote["templateID"] == template
        && metadata.as_object().is_some_and(|fields| {
            fields
                .iter()
                .all(|(key, value)| remote["metadata"].get(key) == Some(value))
        })
}

async fn release(app: &App, id: &str) -> bool {
    if provider::call(
        app,
        &format!("/sandboxes/{id}"),
        reqwest::Method::DELETE,
        None,
    )
    .await
    .is_err()
    {
        return false;
    }
    matches!(
        provider::call(app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await,
        Ok(Value::Null)
    )
}

async fn run_code(app: &App, id: &str, quantities: &[i64], expected: &Summary) -> Result<()> {
    // Only validated integers enter this backend-owned program. No model code is executed.
    let code = format!(
        "import json\n_values = {}\njson.dumps({{'count': len(_values), 'total': sum(_values), 'minimum': min(_values), 'maximum': max(_values)}}, separators=(',', ':'))",
        json!(quantities)
    );
    let url = bridge::guest_url(id, "/execute")?;
    let mut response = app
        .http
        .post(url)
        .timeout(std::time::Duration::from_secs(30))
        .json(&json!({"code":code,"language":"python","env_vars":null}))
        .send()
        .await
        .map_err(|_| provider::unavailable())?;
    if !response.status().is_success() {
        return Err(provider::unavailable());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| provider::unavailable())?
    {
        if body.len() + chunk.len() > MAX_OUTPUT {
            return Err(provider::unavailable());
        }
        body.extend_from_slice(&chunk);
    }
    let mut result = None;
    let mut ended = false;
    for line in body
        .split(|byte| *byte == b'\n')
        .filter(|line| !line.is_empty())
    {
        let event: Value = serde_json::from_slice(line).map_err(|_| provider::unavailable())?;
        match event["type"].as_str() {
            Some("error" | "stderr") => return Err(provider::unavailable()),
            Some("result") if event["is_main_result"] == true => {
                if result.is_some() {
                    return Err(provider::unavailable());
                }
                result = event["text"].as_str().map(str::to_owned);
            }
            Some("end_of_execution") => ended = true,
            _ => {}
        }
    }
    let actual: Summary = serde_json::from_str(
        result
            .as_deref()
            .filter(|_| ended)
            .ok_or_else(provider::unavailable)?,
    )
    .map_err(|_| provider::unavailable())?;
    if &actual != expected {
        return Err(Error::conflict("cube_tool_result_mismatch"));
    }
    Ok(())
}

async fn record_result(
    app: &App,
    operation: Uuid,
    sandbox_id: Option<&str>,
    result: Option<&Value>,
    error_code: Option<&str>,
    cleanup_confirmed: bool,
) -> Result<()> {
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let state = if result.is_some() && cleanup_confirmed {
        "completed"
    } else {
        "failed"
    };
    let changed = tx.execute(
        "UPDATE cube_quantity_tool_calls SET sandbox_id=COALESCE($2,sandbox_id),state=$3,result=$4,error_code=$5,cleanup_confirmed=$6,updated_at=now() WHERE id=$1 AND state<>'completed'",
        &[&operation, &sandbox_id, &state, &result, &error_code, &cleanup_confirmed],
    )
    .await?;
    if state == "completed" && changed == 1 {
        tx.execute(
            "INSERT INTO audit_events(tenant_id,case_id,actor_id,event_type,payload) SELECT c.tenant_id,c.case_id,s.owner_id,'agent.cube_tool_completed',jsonb_build_object('operation_id',c.id,'tool_call_id',c.tool_call_id,'template_id',c.template_id,'sandbox_id',c.sandbox_id,'cleanup_confirmed',true) FROM cube_quantity_tool_calls c JOIN agent_sessions s ON s.id=c.session_id WHERE c.id=$1",
            &[&operation],
        ).await?;
    }
    tx.commit().await?;
    Ok(())
}

pub async fn summarize_quantities(
    State(app): State<App>,
    Path(session): Path<String>,
    headers: HeaderMap,
    Json(input): Json<QuantityInput>,
) -> Result<Json<Value>> {
    let expected = validate(&input)?;
    let template = template_id()?;
    let token = crate::auth::bearer(&headers)?;
    let operation = Uuid::new_v4();
    let digest = json_digest(&json!(input.quantities));
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    agent_sessions::authorize(&tx, &session, &token, false).await?;
    let scope = tx
        .query_opt(
            "SELECT s.tenant_id,s.owner_id,s.case_id,r.message_id FROM agent_sessions s JOIN case_conversations c ON c.session_id=s.id AND c.tenant_id=s.tenant_id AND c.case_id=s.case_id JOIN agent_runs r ON r.case_id=c.case_id AND r.tenant_id=c.tenant_id WHERE s.id=$1 AND r.state='running' AND NOT r.cancel_requested",
            &[&session],
        )
        .await?
        .ok_or_else(Error::forbidden)?;
    let tenant: Uuid = scope.get(0);
    let actor: Uuid = scope.get(1);
    let case: Uuid = scope.get(2);
    let message: i64 = scope.get(3);
    let inserted = tx
        .query_opt(
            "INSERT INTO cube_quantity_tool_calls(id,tenant_id,case_id,session_id,message_id,tool_call_id,input_digest,template_id,state) VALUES($1,$2,$3,$4,$5,$6,$7,$8,'creating') ON CONFLICT(session_id,tool_call_id) DO NOTHING RETURNING id",
            &[&operation, &tenant, &case, &session, &message, &input.tool_call_id, &digest, &template],
        )
        .await?
        .is_some();
    if !inserted {
        let row = tx
            .query_one(
                "SELECT message_id,input_digest,template_id,state,result FROM cube_quantity_tool_calls WHERE session_id=$1 AND tool_call_id=$2",
                &[&session, &input.tool_call_id],
            )
            .await?;
        if row.get::<_, i64>(0) != message
            || row.get::<_, String>(1) != digest
            || row.get::<_, String>(2) != template
        {
            return Err(Error::conflict("cube_tool_call_mismatch"));
        }
        let state: String = row.get(3);
        let result: Option<Value> = row.get(4);
        tx.commit().await?;
        return match (state.as_str(), result) {
            ("completed", Some(value)) => Ok(Json(value)),
            ("failed", _) => Err(Error::conflict("cube_tool_failed")),
            _ => Err(Error::conflict("cube_tool_in_progress")),
        };
    }
    tx.commit().await?;
    drop(db);

    let mut sandbox_id = None;
    let mut creation_attempted = false;
    let outcome = async {
        provider::preflight(&app, &template).await?;
        let metadata = json!({"project":"erp-agent","erp_tool_operation":operation,
            "erp_tenant":tenant,"erp_case":case,"erp_session":session,"erp_message":message.to_string(),"erp_user":actor,"erp_tool":TOOL});
        let request = json!({"templateID":template,"timeout":180,"allow_internet_access":false,
            "metadata":metadata,"network":{"allow_out":[]}});
        creation_attempted = true;
        let created = match provider::call(&app, "/sandboxes", reqwest::Method::POST, Some(request)).await {
            Ok(value) => value,
            Err(error) => match find_created(&app, operation, &template).await? {
                Some(id) => provider::call(&app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await?,
                None => return Err(error),
            },
        };
        let id = provider::remote_id(&created)?.to_owned();
        let remote = provider::call(&app, &format!("/sandboxes/{id}"), reqwest::Method::GET, None).await?;
        if !matches_scope(&remote, &metadata, &template) {
            return Err(Error::conflict("cube_tool_scope_mismatch"));
        }
        sandbox_id = Some(id.clone());
        let db = app.db.get().await?;
        db.execute("UPDATE cube_quantity_tool_calls SET sandbox_id=$2,state='running',updated_at=now() WHERE id=$1", &[&operation,&id]).await?;
        drop(db);
        run_code(&app, &id, &input.quantities, &expected).await?;
        Ok(())
    }
    .await;
    if sandbox_id.is_none() {
        match find_created(&app, operation, &template).await {
            Ok(found) => sandbox_id = found,
            Err(_) => {
                record_result(
                    &app,
                    operation,
                    None,
                    None,
                    Some("cube_tool_cleanup_unconfirmed"),
                    false,
                )
                .await?;
                return Err(Error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "cube_tool_cleanup_unconfirmed",
                    "Quantity tool cleanup could not be confirmed",
                ));
            }
        }
    }
    let cleaned = match sandbox_id.as_deref() {
        Some(id) => release(&app, id).await,
        // A lost create acknowledgement can precede inventory visibility.
        None => !creation_attempted,
    };
    let result = json!({"tool":TOOL,"template_id":template,"sandbox_id":sandbox_id,
        "count":expected.count,"total":expected.total,"minimum":expected.minimum,"maximum":expected.maximum,
        "cleanup_confirmed":cleaned});
    let error_code = if !cleaned && sandbox_id.is_none() {
        Some("cube_tool_cleanup_unconfirmed")
    } else if !cleaned {
        Some("cube_tool_cleanup_failed")
    } else {
        outcome.as_ref().err().map(|error| error.1)
    };
    record_result(
        &app,
        operation,
        sandbox_id.as_deref(),
        if error_code.is_none() {
            Some(&result)
        } else {
            None
        },
        error_code,
        cleaned,
    )
    .await?;
    if let Some(code) = error_code {
        return Err(Error(
            StatusCode::SERVICE_UNAVAILABLE,
            code,
            "Quantity tool execution failed",
        ));
    }
    Ok(Json(result))
}

pub(super) async fn sweep_stale(app: &App) {
    let Ok(db) = app.db.get().await else { return };
    let Ok(rows) = db.query("SELECT id,template_id,sandbox_id FROM cube_quantity_tool_calls WHERE created_at<now()-interval '90 seconds' AND (state IN ('creating','running') OR NOT cleanup_confirmed) ORDER BY updated_at LIMIT 20", &[]).await else { return };
    drop(db);
    for row in rows {
        let operation: Uuid = row.get(0);
        // Rotate unresolved operations so one lost creation cannot starve cleanup.
        if let Ok(db) = app.db.get().await {
            let _ = db
                .execute(
                    "UPDATE cube_quantity_tool_calls SET updated_at=now() WHERE id=$1",
                    &[&operation],
                )
                .await;
        }
        let template: String = row.get(1);
        let id: Option<String> = row.get(2);
        let id = match id {
            Some(id) => Some(id),
            None => match find_created(app, operation, &template).await {
                Ok(found) => found,
                Err(_) => continue,
            },
        };
        let cleaned = match id.as_deref() {
            Some(id) => release(app, id).await,
            None => false,
        };
        if cleaned {
            let _ = record_result(
                app,
                operation,
                id.as_deref(),
                None,
                Some("cube_tool_interrupted"),
                true,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_contract_rejects_unbounded_or_unsafe_inputs() {
        let mut input = QuantityInput {
            tool_call_id: "call_1".into(),
            quantities: vec![12, 18, 20],
        };
        assert_eq!(validate(&input).unwrap().total, 50);
        for bad in [vec![], vec![-1], vec![1_000_001], vec![0; 33]] {
            input.quantities = bad;
            assert!(validate(&input).is_err());
        }
        input.quantities = vec![1];
        input.tool_call_id = "../other".into();
        assert!(validate(&input).is_err());
    }
}
