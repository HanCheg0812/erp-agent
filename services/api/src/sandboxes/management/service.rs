use super::domain::{
    Activate, AdoptTemplate, CreateCandidate, adopted_spec, cube_build_request, valid_id,
    validate_name, validate_spec,
};
use crate::{auth::Principal, cases, platform::*};
use serde_json::{Value, json};
use tokio_postgres::GenericClient;
use uuid::Uuid;

const CANDIDATE: &str = "SELECT to_jsonb(c) FROM runtime_candidates c WHERE c.id=$1";

pub(in crate::sandboxes) struct Selection {
    pub(in crate::sandboxes) template_id: String,
    pub(in crate::sandboxes) candidate_id: Option<Uuid>,
    pub(in crate::sandboxes) release_id: Option<Uuid>,
    pub(in crate::sandboxes) version: i64,
}

pub(in crate::sandboxes) async fn selection<C: GenericClient + Sync>(
    db: &C,
    case_id: Uuid,
) -> Result<Selection> {
    if let Some(row) = db.query_opt("SELECT c.id,c.template_id FROM runtime_canaries x JOIN runtime_candidates c ON c.id=x.candidate_id WHERE x.case_id=$1 AND c.state='ready'", &[&case_id]).await? {
        return Ok(Selection { template_id: row.get(1), candidate_id: Some(row.get(0)), release_id: None, version: 0 });
    }
    let control = db
        .query_one(
            "SELECT active_release_id,version FROM runtime_control WHERE singleton=true FOR SHARE",
            &[],
        )
        .await?;
    let release_id: Option<Uuid> = control.get(0);
    let version: i64 = control.get(1);
    if let Some(release_id) = release_id {
        let row = db
            .query_one(
                "SELECT candidate_id,template_id FROM runtime_releases WHERE id=$1",
                &[&release_id],
            )
            .await?;
        return Ok(Selection {
            template_id: row.get(1),
            candidate_id: Some(row.get(0)),
            release_id: Some(release_id),
            version,
        });
    }
    let template_id = env("CUBE_TEMPLATE_ID", "");
    valid_id(&template_id)?;
    Ok(Selection {
        template_id,
        candidate_id: None,
        release_id: None,
        version,
    })
}

/// Resolve a template that has been explicitly adopted into the Forge
/// runtime catalog. Cube templates remain provider-owned; this catalog is the
/// backend allowlist that prevents a model or browser request from selecting
/// an arbitrary provider identifier.
pub(in crate::sandboxes) async fn selection_for_template<C: GenericClient + Sync>(
    db: &C,
    template_id: &str,
) -> Result<Selection> {
    let template_id = valid_id(template_id)?.to_owned();
    if let Some(row) = db
        .query_opt(
            "SELECT c.id,r.id,rc.version FROM runtime_candidates c LEFT JOIN LATERAL (SELECT id FROM runtime_releases WHERE candidate_id=c.id ORDER BY created_at DESC LIMIT 1) r ON true CROSS JOIN runtime_control rc WHERE c.template_id=$1 AND c.state='ready' ORDER BY (r.id IS NOT NULL) DESC,c.updated_at DESC LIMIT 1",
            &[&template_id],
        )
        .await?
    {
        return Ok(Selection {
            template_id,
            candidate_id: Some(row.get(0)),
            release_id: row.get(1),
            version: row.get(2),
        });
    }
    if template_id == env("CUBE_TEMPLATE_ID", "") {
        let version: i64 = db
            .query_one(
                "SELECT version FROM runtime_control WHERE singleton=true",
                &[],
            )
            .await?
            .get(0);
        return Ok(Selection {
            template_id,
            candidate_id: None,
            release_id: None,
            version,
        });
    }
    Err(Error::conflict("template_not_adopted"))
}

fn sanitized_template(value: &Value) -> Value {
    json!({"template_id":value["templateID"],"status":value["status"],"image":value["imageInfo"],"created_at":value["createdAt"],"aliases":value["aliases"]})
}

async fn nodes(app: &App) -> Result<Value> {
    let url = format!(
        "{}/internal/v1/nodes",
        env("CUBE_OPS_URL", "http://127.0.0.1:3010").trim_end_matches('/')
    );
    app.http
        .get(url)
        .timeout(std::time::Duration::from_secs(8))
        .send()
        .await
        .map_err(|_| super::super::provider::unavailable())?
        .error_for_status()
        .map_err(|_| super::super::provider::unavailable())?
        .json()
        .await
        .map_err(|_| super::super::provider::unavailable())
}

async fn canary_evidence<C: GenericClient + Sync>(db: &C, candidate_id: Uuid) -> Result<Value> {
    let canary = db
        .query_opt(
            "SELECT to_jsonb(x) FROM runtime_canaries x WHERE candidate_id=$1",
            &[&candidate_id],
        )
        .await?
        .map(|row| row.get::<_, Value>(0));
    let Some(canary) = canary else {
        return Ok(json!({"ready":false,"reason":"No test conversation"}));
    };
    let case_id = uuid(canary["case_id"].as_str().unwrap())?;
    let passed: bool = db.query_one("SELECT EXISTS(SELECT 1 FROM agent_runs r JOIN case_sandboxes s ON s.tenant_id=r.tenant_id AND s.case_id=r.case_id JOIN runtime_candidates c ON c.id=$2 WHERE r.case_id=$1 AND r.state='completed' AND r.operation='message' AND r.finished_at>=s.created_at AND r.timings @> '[{\"backend\":\"cube\",\"tools\":[{\"name\":\"case_read\",\"failed\":false}]}]'::jsonb AND r.tools @> '[{\"name\":\"case_read\",\"state\":\"completed\"}]'::jsonb AND s.runtime_candidate_id=c.id AND s.runtime_template_id=c.template_id AND s.last_sandbox_id IS NOT NULL)", &[&case_id,&candidate_id]).await?.get(0);
    Ok(
        json!({"case_id":case_id,"ready":passed,"reason":if passed { "Verified Cube turn and case_read" } else { "Run a Cube-resident turn with a successful case_read tool call" }}),
    )
}

async fn candidate_value<C: GenericClient + Sync>(db: &C, id: Uuid) -> Result<Value> {
    let mut value = object(db, CANDIDATE, &[&id]).await?;
    value["build_request"] = if value["source"] == "build" {
        cube_build_request(&value)
    } else {
        Value::Null
    };
    value["canary"] = canary_evidence(db, id).await?;
    Ok(value)
}

async fn refresh_candidate(app: &App, id: Uuid) -> Result<()> {
    let db = app.db.get().await?;
    let row = object(&**db, CANDIDATE, &[&id]).await?;
    if row["state"] != "building" {
        return Ok(());
    }
    let Some(template_id) = row["template_id"].as_str() else {
        return Ok(());
    };
    let detail = super::super::provider::call(
        app,
        &format!("/templates/{template_id}"),
        reqwest::Method::GET,
        None,
    )
    .await?;
    let state = match detail["status"].as_str() {
        Some("READY") => "ready",
        Some("FAILED") => "failed",
        _ => "building",
    };
    db.execute("UPDATE runtime_candidates SET state=$2,error_code=CASE WHEN $2='failed' THEN 'cube_build_failed' ELSE error_code END,updated_at=now() WHERE id=$1 AND state='building'", &[&id,&state]).await?;
    Ok(())
}

pub(super) async fn overview(app: &App, actor: &Principal) -> Result<Value> {
    actor.require("runtime_admin")?;
    let db = app.db.get().await?;
    let control = db
        .query_one(
            "SELECT active_release_id,version FROM runtime_control WHERE singleton=true",
            &[],
        )
        .await?;
    let candidates = array(
        &**db,
        "SELECT to_jsonb(c) FROM runtime_candidates c ORDER BY created_at DESC LIMIT 100",
        &[],
    )
    .await?;
    let releases = array(
        &**db,
        "SELECT to_jsonb(r) FROM runtime_releases r ORDER BY created_at DESC LIMIT 30",
        &[],
    )
    .await?;
    let instances = array(&**db, "SELECT jsonb_build_object('id',s.id,'template_id',s.runtime_template_id,'candidate_id',s.runtime_candidate_id,'release_id',s.runtime_release_id,'sandbox_id',s.sandbox_id,'state',s.state,'active',s.active,'guest_ready',s.guest_ready,'created_at',s.created_at,'checked_at',s.checked_at,'error_code',s.error_code) FROM case_sandboxes s WHERE s.state<>'released' ORDER BY s.created_at DESC LIMIT 100", &[]).await?;
    let templates =
        super::super::provider::call(app, "/templates", reqwest::Method::GET, None).await;
    let node_sample = nodes(app).await;
    let runtime_error = templates.is_err() || node_sample.is_err();
    let templates = templates
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(sanitized_template)
        .collect::<Vec<_>>();
    let nodes = node_sample.ok().and_then(|v| v.as_array().cloned()).unwrap_or_default().iter().map(|v| json!({"ip":v["IP"],"healthy":v["Healthy"],"status":v["HostStatus"],"cpu_total":v["CpuTotal"],"memory_mib":v["MemMBTotal"],"disk_percent":v["StorageDiskUsagePer"],"sampled_at":v["MetricUpdateAt"]})).collect::<Vec<_>>();
    Ok(
        json!({"active_release_id":control.get::<_,Option<Uuid>>(0),"version":control.get::<_,i64>(1),"legacy_template_id":env("CUBE_TEMPLATE_ID",""),"candidates":candidates,"releases":releases,"instances":instances,"templates":templates,"nodes":nodes,"sampled_at":chrono::DateTime::<chrono::Utc>::from(std::time::SystemTime::now()).to_rfc3339(),"provider_available":!runtime_error}),
    )
}

pub(super) async fn create_candidate(app: &App, actor: &Principal, input: Value) -> Result<Value> {
    actor.require("runtime_admin")?;
    let input: CreateCandidate = parse(input)?;
    validate_spec(&input)?;
    let db = app.db.get().await?;
    db.execute("INSERT INTO runtime_candidates(id,name,image,cpu_milli,memory_mib,state,source,created_by) VALUES($1,$2,$3,$4,$5,'draft','build',$6) ON CONFLICT (id) DO NOTHING", &[&input.request_id,&input.name,&input.image,&input.cpu_milli,&input.memory_mib,&actor.user_id]).await?;
    let value = candidate_value(&**db, input.request_id).await?;
    if value["created_by"] != actor.user_id.to_string()
        || value["name"] != input.name
        || value["image"] != input.image
        || value["cpu_milli"] != input.cpu_milli
        || value["memory_mib"] != input.memory_mib
    {
        return Err(Error::conflict("idempotency_conflict"));
    }
    Ok(value)
}

pub(super) async fn candidate(app: &App, actor: &Principal, id: Uuid) -> Result<Value> {
    actor.require("runtime_admin")?;
    refresh_candidate(app, id).await?;
    let db = app.db.get().await?;
    candidate_value(&**db, id).await
}

pub(super) async fn build_candidate(app: &App, actor: &Principal, id: Uuid) -> Result<Value> {
    actor.require("runtime_admin")?;
    let db = app.db.get().await?;
    let updated = db.execute("UPDATE runtime_candidates SET state='requested',updated_at=now() WHERE id=$1 AND state='draft' AND source='build'", &[&id]).await?;
    if updated == 0 {
        return Err(Error::conflict("build_already_requested"));
    }
    let value = object(&**db, CANDIDATE, &[&id]).await?;
    let job = super::super::provider::call(
        app,
        "/templates",
        reqwest::Method::POST,
        Some(cube_build_request(&value)),
    )
    .await;
    match job {
        Ok(job) => {
            let ids = valid_id(job["templateID"].as_str().unwrap_or(""))
                .and_then(|template| valid_id(job["jobID"].as_str().unwrap_or("")).map(|build| (template, build)));
            if let Ok((template, build)) = ids {
                db.execute("UPDATE runtime_candidates SET state='building',template_id=$2,job_id=$3,updated_at=now() WHERE id=$1", &[&id,&template,&build]).await?;
            } else {
                db.execute("UPDATE runtime_candidates SET state='unconfirmed',error_code='cube_acknowledgement_invalid',updated_at=now() WHERE id=$1", &[&id]).await?;
            }
        }
        Err(_) => {
            db.execute("UPDATE runtime_candidates SET state='unconfirmed',error_code='cube_acknowledgement_unknown',updated_at=now() WHERE id=$1", &[&id]).await?;
        }
    }
    candidate_value(&**db, id).await
}

pub(super) async fn adopt_template(
    app: &App,
    actor: &Principal,
    id: &str,
    input: Value,
) -> Result<Value> {
    actor.require("runtime_admin")?;
    let id = valid_id(id)?;
    let input: AdoptTemplate = parse(input)?;
    validate_name(&input.name)?;
    let summaries =
        super::super::provider::call(app, "/templates", reqwest::Method::GET, None).await?;
    let summary = summaries
        .as_array()
        .and_then(|list| list.iter().find(|item| item["templateID"] == id))
        .ok_or_else(Error::missing)?;
    let detail =
        super::super::provider::call(app, &format!("/templates/{id}"), reqwest::Method::GET, None)
            .await?;
    let (image, cpu, memory) = adopted_spec(&detail, summary)?;
    super::super::provider::preflight(app, id).await?;
    let db = app.db.get().await?;
    if let Some(existing) = db
        .query_opt(
            "SELECT id FROM runtime_candidates WHERE template_id=$1",
            &[&id],
        )
        .await?
    {
        if existing.get::<_, Uuid>(0) != input.request_id {
            return Err(Error::conflict("template_already_adopted"));
        }
    }
    db.execute("INSERT INTO runtime_candidates(id,name,image,cpu_milli,memory_mib,state,source,template_id,created_by) VALUES($1,$2,$3,$4,$5,'ready','adopt',$6,$7) ON CONFLICT DO NOTHING", &[&input.request_id,&input.name,&image,&cpu,&memory,&id,&actor.user_id]).await?;
    if db
        .query_opt(
            "SELECT id FROM runtime_candidates WHERE id=$1",
            &[&input.request_id],
        )
        .await?
        .is_none()
    {
        return Err(Error::conflict("template_already_adopted"));
    }
    let value = candidate_value(&**db, input.request_id).await?;
    if value["template_id"] != id
        || value["image"] != image
        || value["created_by"] != actor.user_id.to_string()
    {
        return Err(Error::conflict("idempotency_conflict"));
    }
    Ok(value)
}

pub(super) async fn create_canary(app: &App, actor: &Principal, id: Uuid) -> Result<Value> {
    actor.require("runtime_admin")?;
    actor.require("planner")?;
    let mut db = app.db.get().await?;
    let tx = db.transaction().await?;
    let row = tx
        .query_one(
            "SELECT state,template_id FROM runtime_candidates WHERE id=$1 FOR UPDATE",
            &[&id],
        )
        .await?;
    if row.get::<_, String>(0) != "ready" {
        return Err(Error::conflict("candidate_not_ready"));
    }
    if let Some(row) = tx
        .query_opt(
            "SELECT case_id,tenant_id,actor_id FROM runtime_canaries WHERE candidate_id=$1",
            &[&id],
        )
        .await?
    {
        if row.get::<_, Uuid>(1) != actor.tenant_id || row.get::<_, Uuid>(2) != actor.user_id {
            return Err(Error::forbidden());
        }
        return Ok(json!({"case_id":row.get::<_,Uuid>(0)}));
    }
    let record = object(&*tx,"INSERT INTO cases(tenant_id,owner_id,title,kind,quantity_basis,conversation_request_id) VALUES($1,$2,'Runtime validation','conversation','unknown',$3) RETURNING to_jsonb(cases)",&[&actor.tenant_id,&actor.user_id,&id]).await?;
    let case_id = uuid(record["id"].as_str().unwrap())?;
    tx.execute(
        "INSERT INTO runtime_canaries(candidate_id,tenant_id,case_id,actor_id) VALUES($1,$2,$3,$4)",
        &[&id, &actor.tenant_id, &case_id, &actor.user_id],
    )
    .await?;
    cases::audit(
        &*tx,
        actor,
        case_id,
        "runtime.canary.created",
        json!({"candidate_id":id}),
    )
    .await?;
    tx.commit().await?;
    Ok(json!({"case_id":case_id}))
}

pub(super) async fn activate_candidate(
    app: &App,
    actor: &Principal,
    id: Uuid,
    input: Value,
) -> Result<Value> {
    actor.require("runtime_admin")?;
    let input: Activate = parse(input)?;
    let mut db = app.db.get().await?;
    let row = object(&**db, CANDIDATE, &[&id]).await?;
    if row["state"] != "ready" {
        return Err(Error::conflict("candidate_not_ready"));
    }
    let template = row["template_id"].as_str().ok_or_else(Error::missing)?;
    super::super::provider::preflight(app, template).await?;
    let tx = db.transaction().await?;
    let version: i64 = tx
        .query_one(
            "SELECT version FROM runtime_control WHERE singleton=true FOR UPDATE",
            &[],
        )
        .await?
        .get(0);
    if version != input.expected_version {
        return Err(Error::conflict("runtime_version_changed"));
    }
    if canary_evidence(&*tx, id).await?["ready"] != true {
        return Err(Error::conflict("canary_not_verified"));
    }
    let release_id = Uuid::new_v4();
    tx.execute("INSERT INTO runtime_releases(id,candidate_id,template_id,activated_by) VALUES($1,$2,$3,$4)", &[&release_id,&id,&template,&actor.user_id]).await?;
    tx.execute(
        "UPDATE runtime_control SET active_release_id=$1,version=version+1 WHERE singleton=true",
        &[&release_id],
    )
    .await?;
    tx.commit().await?;
    Ok(
        json!({"release_id":release_id,"candidate_id":id,"template_id":template,"version":version+1}),
    )
}
