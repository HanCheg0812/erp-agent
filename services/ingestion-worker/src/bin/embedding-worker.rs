use anyhow::{Context, Result, bail};
use erp_ingestion::embedding::{TeiClient, content_hash, vector_literal};
use std::time::Duration;
use tokio_postgres::{Client, NoTls, Row};
use uuid::Uuid;

#[derive(Debug)]
struct Job {
    id: Uuid,
    lease_token: Uuid,
    chunk_id: Uuid,
    model_id: String,
    operation: String,
    content_hash: String,
    text: Option<String>,
    attempt_count: i32,
}

struct Config {
    model_id: String,
    batch_size: i64,
    lease_seconds: i64,
    poll: Duration,
    timeout: Duration,
}

#[tokio::main]
async fn main() -> Result<()> {
    dotenvy::dotenv().ok();
    let config = Config::from_env()?;
    let database_url = required_env("DATABASE_URL")?;
    let tei_url = std::env::var("RAG_EMBEDDING_URL")
        .or_else(|_| std::env::var("TEI_URL"))
        .context("RAG_EMBEDDING_URL or TEI_URL must be configured")?;
    let tei = TeiClient::new(
        tei_url,
        std::env::var("TEI_API_KEY").ok().as_deref(),
        &config.model_id,
        config.timeout,
        env_bool("RAG_EMBEDDING_NORMALIZE", true),
    )?;

    let (db, connection) = tokio_postgres::connect(&database_url, NoTls)
        .await
        .context("connect embedding worker database")?;
    tokio::spawn(async move {
        if let Err(error) = connection.await {
            eprintln!("embedding worker postgres connection stopped: {error:#}");
        }
    });

    let model_name = ensure_model(&db, &config.model_id).await?;
    ensure_vector_table(&db).await?;
    tei.health().await?;
    let info = tei.info().await?;
    if info.model_id.as_deref() != Some(model_name.as_str()) {
        bail!(
            "TEI serves {:?}, but embedding model {:?} is registered as {:?}",
            info.model_id,
            config.model_id,
            model_name
        );
    }
    eprintln!(
        "embedding worker ready: model={} tei_model={:?} max_input_length={:?}",
        config.model_id, info.model_id, info.max_input_length
    );

    loop {
        let jobs = claim_jobs(&db, &config).await?;
        if jobs.is_empty() {
            tokio::select! {
                _ = tokio::signal::ctrl_c() => break,
                _ = tokio::time::sleep(config.poll) => {}
            }
            continue;
        }
        process_jobs(&db, &tei, &config.model_id, jobs).await?;
    }

    eprintln!("embedding worker stopped");
    Ok(())
}

impl Config {
    fn from_env() -> Result<Self> {
        let model_id = required_env("RAG_EMBEDDING_MODEL_ID")?;
        let batch_size = env_i64("RAG_EMBEDDING_BATCH_SIZE", 16, 1, 128)?;
        let lease_seconds = env_i64("RAG_EMBEDDING_LEASE_SECONDS", 300, 30, 86_400)?;
        let poll_ms = env_i64("RAG_EMBEDDING_POLL_MS", 1_000, 50, 60_000)?;
        let timeout_seconds = env_i64("RAG_EMBEDDING_TIMEOUT_SECONDS", 60, 1, 600)?;
        Ok(Self {
            model_id,
            batch_size,
            lease_seconds,
            poll: Duration::from_millis(poll_ms as u64),
            timeout: Duration::from_secs(timeout_seconds as u64),
        })
    }
}

async fn ensure_model(db: &Client, model_id: &str) -> Result<String> {
    let row = db
        .query_opt(
            "SELECT model_name, dimensions, active FROM embedding_models WHERE id = $1",
            &[&model_id],
        )
        .await
        .context("read embedding model registry")?
        .with_context(|| format!("embedding model {model_id:?} is not registered"))?;
    let active: bool = row.get("active");
    if !active {
        bail!("embedding model {model_id:?} is inactive");
    }
    let dimensions: i32 = row.get("dimensions");
    if !(1..=16_000).contains(&dimensions) {
        bail!("embedding model {model_id:?} has invalid dimensions {dimensions}");
    }
    Ok(row.get("model_name"))
}

async fn ensure_vector_table(db: &Client) -> Result<()> {
    let exists: Option<String> = db
        .query_one(
            "SELECT to_regclass('public.knowledge_chunk_embeddings')",
            &[],
        )
        .await
        .context("check knowledge embedding table")?
        .get(0);
    if exists.is_none() {
        bail!("knowledge_chunk_embeddings is unavailable; apply the pgvector migration first");
    }
    Ok(())
}

async fn claim_jobs(db: &Client, config: &Config) -> Result<Vec<Job>> {
    let token = Uuid::new_v4();
    let rows = db
        .query(
            "WITH next AS (
               SELECT j.id
               FROM knowledge_embedding_jobs j
               WHERE j.model_id = $1
                 AND (
                   (j.state = 'pending' AND j.available_at <= clock_timestamp())
                   OR (j.state = 'failed' AND j.available_at <= clock_timestamp())
                   OR (j.state = 'processing' AND j.lease_until < clock_timestamp())
                 )
               ORDER BY j.available_at, j.created_at, j.id
               FOR UPDATE SKIP LOCKED
               LIMIT $2
             )
             UPDATE knowledge_embedding_jobs j
             SET state = 'processing',
                 attempt_count = j.attempt_count + 1,
                 lease_until = clock_timestamp() + ($3::bigint * interval '1 second'),
                 lease_token = $4,
                 last_error = NULL
             FROM next
             WHERE j.id = next.id
             RETURNING j.id, j.chunk_id, j.model_id, j.operation,
                       j.content_hash, j.attempt_count,
                       $4 AS lease_token,
                       (SELECT c.text FROM knowledge_chunks c WHERE c.id = j.chunk_id) AS chunk_text",
            &[&config.model_id, &config.batch_size, &config.lease_seconds, &token],
        )
        .await
        .context("claim knowledge embedding jobs")?;
    rows.into_iter()
        .map(job_from_row)
        .collect::<Result<Vec<_>>>()
}

fn job_from_row(row: Row) -> Result<Job> {
    Ok(Job {
        id: row.get("id"),
        lease_token: row.get("lease_token"),
        chunk_id: row.get("chunk_id"),
        model_id: row.get("model_id"),
        operation: row.get("operation"),
        content_hash: row.get("content_hash"),
        text: row.get("chunk_text"),
        attempt_count: row.get("attempt_count"),
    })
}

async fn process_jobs(db: &Client, tei: &TeiClient, model_id: &str, jobs: Vec<Job>) -> Result<()> {
    let mut embed_jobs = Vec::new();
    for job in jobs {
        if job.operation == "retract" {
            retract_job(db, &job).await?;
            continue;
        }
        if job.operation != "embed" {
            fail_job(db, &job, format!("unsupported operation {}", job.operation)).await?;
            continue;
        }
        let Some(text) = job.text.clone() else {
            fail_job(db, &job, "knowledge chunk is missing".into()).await?;
            continue;
        };
        if content_hash(&text) != job.content_hash {
            fail_job(db, &job, "knowledge chunk content hash changed".into()).await?;
            continue;
        }
        embed_jobs.push((job, text));
    }

    if embed_jobs.is_empty() {
        return Ok(());
    }
    let inputs = embed_jobs
        .iter()
        .map(|(_, text)| text.clone())
        .collect::<Vec<_>>();
    let vectors = match tei.embed(&inputs).await {
        Ok(vectors) => vectors,
        Err(error) => {
            for (job, _) in &embed_jobs {
                fail_job(db, job, format!("TEI embedding failed: {error:#}")).await?;
            }
            return Ok(());
        }
    };

    let dimensions = vectors.first().map_or(0, Vec::len) as i32;
    let registered: i32 = db
        .query_one(
            "SELECT dimensions FROM embedding_models WHERE id = $1",
            &[&model_id],
        )
        .await
        .context("read embedding dimensions")?
        .get(0);
    if dimensions != registered {
        for (job, _) in &embed_jobs {
            fail_job(
                db,
                job,
                format!("TEI returned {dimensions} dimensions; registry requires {registered}"),
            )
            .await?;
        }
        return Ok(());
    }

    for ((job, _), vector) in embed_jobs.into_iter().zip(vectors) {
        let literal = match vector_literal(&vector) {
            Ok(literal) => literal,
            Err(error) => {
                fail_job(db, &job, format!("invalid embedding: {error:#}")).await?;
                continue;
            }
        };
        let inserted = match db
            .execute(
                "INSERT INTO knowledge_chunk_embeddings
                   (chunk_id, model_id, dimensions, embedding, embedded_text_hash)
                 SELECT $1, $2, $3, $4::vector, $5
                 WHERE EXISTS (
                   SELECT 1 FROM knowledge_embedding_jobs
                   WHERE id = $6 AND state = 'processing' AND lease_token = $7
                 )
                 ON CONFLICT (chunk_id, model_id) DO UPDATE
                   SET dimensions = EXCLUDED.dimensions,
                       embedding = EXCLUDED.embedding,
                       embedded_text_hash = EXCLUDED.embedded_text_hash,
                       created_at = clock_timestamp()",
                &[
                    &job.chunk_id,
                    &model_id,
                    &dimensions,
                    &literal,
                    &job.content_hash,
                    &job.id,
                    &job.lease_token,
                ],
            )
            .await
        {
            Ok(count) => count,
            Err(error) => {
                fail_job(db, &job, format!("store embedding failed: {error}")).await?;
                continue;
            }
        };
        if inserted == 0 {
            eprintln!(
                "embedding job {} lost its lease before storing vector",
                job.id
            );
            continue;
        }
        let completed = db
            .execute(
                "UPDATE knowledge_embedding_jobs
                 SET state = 'completed', processed_at = clock_timestamp(),
                     lease_until = NULL, lease_token = NULL
                 WHERE id = $1 AND state = 'processing' AND lease_token = $2",
                &[&job.id, &job.lease_token],
            )
            .await
            .context("complete knowledge embedding job")?;
        if completed != 1 {
            eprintln!(
                "embedding job {} lost its lease after storing vector",
                job.id
            );
        }
    }
    Ok(())
}

async fn retract_job(db: &Client, job: &Job) -> Result<()> {
    db.execute(
        "DELETE FROM knowledge_chunk_embeddings
         WHERE chunk_id = $1 AND model_id = $2
           AND EXISTS (
             SELECT 1 FROM knowledge_embedding_jobs
             WHERE id = $3 AND state = 'processing' AND lease_token = $4
           )",
        &[&job.chunk_id, &job.model_id, &job.id, &job.lease_token],
    )
    .await
    .with_context(|| format!("retract embedding job {}", job.id))?;
    db.execute(
        "UPDATE knowledge_embedding_jobs
         SET state = 'completed', processed_at = clock_timestamp(),
             lease_until = NULL, lease_token = NULL
         WHERE id = $1 AND state = 'processing' AND lease_token = $2",
        &[&job.id, &job.lease_token],
    )
    .await
    .with_context(|| format!("complete retract job {}", job.id))?;
    Ok(())
}

async fn fail_job(db: &Client, job: &Job, error: String) -> Result<()> {
    let error = error.chars().take(1_000).collect::<String>();
    let backoff = 2_i64.pow((job.attempt_count.saturating_sub(1) as u32).min(8));
    db.execute(
        "UPDATE knowledge_embedding_jobs
         SET state = 'failed', last_error = $2,
             available_at = clock_timestamp() + ($3::bigint * interval '1 second'),
             lease_until = NULL, lease_token = NULL
         WHERE id = $1 AND state = 'processing' AND lease_token = $4",
        &[&job.id, &error, &backoff, &job.lease_token],
    )
    .await
    .with_context(|| format!("mark embedding job {} failed", job.id))?;
    Ok(())
}

fn required_env(name: &str) -> Result<String> {
    std::env::var(name).with_context(|| format!("{name} must be configured"))
}

fn env_i64(name: &str, default: i64, min: i64, max: i64) -> Result<i64> {
    let value = std::env::var(name)
        .ok()
        .map(|raw| raw.parse::<i64>())
        .transpose()
        .with_context(|| format!("{name} must be an integer"))?
        .unwrap_or(default);
    if !(min..=max).contains(&value) {
        bail!("{name} must be between {min} and {max}");
    }
    Ok(value)
}

fn env_bool(name: &str, default: bool) -> bool {
    match std::env::var(name).ok().as_deref() {
        Some("0" | "false" | "off") => false,
        Some("1" | "true" | "on") => true,
        _ => default,
    }
}
