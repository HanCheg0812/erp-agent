use anyhow::{Context, Result, bail};
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::time::Duration;
use tokio_postgres::GenericClient;
use uuid::Uuid;

const MAX_ERROR_BODY: usize = 1_000;
const MAX_INPUT_CHARS: usize = 32_000;

#[derive(Clone)]
pub struct TeiClient {
    http: Client,
    base_url: String,
    model_id: String,
    normalize: bool,
}

#[derive(Debug, Serialize)]
struct EmbedRequest<'a> {
    inputs: &'a [String],
    truncate: bool,
    normalize: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeiInfo {
    pub model_id: Option<String>,
    pub model_sha: Option<String>,
    pub dtype: Option<String>,
    #[serde(rename = "model_type")]
    pub model_type: Option<String>,
    pub max_client_batch_size: Option<usize>,
    pub max_input_length: Option<usize>,
}

pub struct KnowledgeChunkInput<'a> {
    pub tenant_id: Uuid,
    pub owner_id: Uuid,
    pub case_id: Uuid,
    pub document_id: Uuid,
    pub source_revision: &'a str,
    pub ordinal: i32,
    pub text: &'a str,
    pub locator: &'a serde_json::Value,
    pub content_hash: &'a str,
    pub model_id: &'a str,
}

pub async fn enqueue_knowledge_chunk<C: GenericClient + Sync>(
    db: &C,
    input: &KnowledgeChunkInput<'_>,
) -> Result<Uuid> {
    if input.text.trim().is_empty() || input.text.chars().count() > 12_000 {
        bail!("knowledge chunk text must be between 1 and 12000 characters");
    }
    if input.content_hash != content_hash(input.text) {
        bail!("knowledge chunk content hash does not match text");
    }
    let row = db
        .query_one(
            "WITH chunk AS (
               INSERT INTO knowledge_chunks
                 (tenant_id, owner_id, case_id, document_id, source_revision,
                  ordinal, text, locator, content_hash, state)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, 'ready')
               ON CONFLICT (document_id, source_revision, ordinal, content_hash)
               DO UPDATE SET text = EXCLUDED.text, locator = EXCLUDED.locator,
                             state = 'ready'
               RETURNING id, tenant_id, owner_id, case_id, content_hash
             )
             INSERT INTO knowledge_embedding_jobs
               (tenant_id, owner_id, case_id, chunk_id, model_id,
                operation, content_hash, state, available_at)
             SELECT tenant_id, owner_id, case_id, id, $10, 'embed',
                    content_hash, 'pending', clock_timestamp()
             FROM chunk
             ON CONFLICT (chunk_id, model_id, operation, content_hash)
             DO UPDATE SET
               state = CASE
                 WHEN knowledge_embedding_jobs.state = 'completed'
                   THEN 'completed'
                 ELSE 'pending'
               END,
               available_at = CASE
                 WHEN knowledge_embedding_jobs.state = 'completed'
                   THEN knowledge_embedding_jobs.available_at
                 ELSE clock_timestamp()
               END,
               last_error = CASE
                 WHEN knowledge_embedding_jobs.state = 'completed'
                   THEN knowledge_embedding_jobs.last_error
                 ELSE NULL
               END,
               lease_until = NULL,
               lease_token = NULL
             RETURNING chunk_id",
            &[
                &input.tenant_id,
                &input.owner_id,
                &input.case_id,
                &input.document_id,
                &input.source_revision,
                &input.ordinal,
                &input.text,
                &input.locator,
                &input.content_hash,
                &input.model_id,
            ],
        )
        .await
        .context("enqueue knowledge chunk embedding")?;
    Ok(row.get("chunk_id"))
}

impl TeiClient {
    pub fn new(
        base_url: impl Into<String>,
        api_key: Option<&str>,
        model_id: impl Into<String>,
        timeout: Duration,
        normalize: bool,
    ) -> Result<Self> {
        let base_url = base_url.into().trim_end_matches('/').to_owned();
        if base_url.is_empty() {
            bail!("TEI_URL must not be empty");
        }

        let mut headers = header::HeaderMap::new();
        if let Some(api_key) = api_key.filter(|value| !value.is_empty()) {
            let value = header::HeaderValue::from_str(&format!("Bearer {api_key}"))
                .context("invalid TEI_API_KEY")?;
            headers.insert(header::AUTHORIZATION, value);
        }

        Ok(Self {
            http: Client::builder()
                .default_headers(headers)
                .timeout(timeout)
                .build()
                .context("build TEI HTTP client")?,
            base_url,
            model_id: model_id.into(),
            normalize,
        })
    }

    pub fn model_id(&self) -> &str {
        &self.model_id
    }

    pub async fn health(&self) -> Result<()> {
        let response = self
            .http
            .get(format!("{}/health", self.base_url))
            .send()
            .await
            .context("request TEI health")?;
        ensure_success(response, "health").await
    }

    pub async fn info(&self) -> Result<TeiInfo> {
        let response = self
            .http
            .get(format!("{}/info", self.base_url))
            .send()
            .await
            .context("request TEI info")?;
        let response = ensure_success_response(response, "info").await?;
        response.json().await.context("decode TEI info")
    }

    pub async fn embed(&self, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        if inputs.is_empty() {
            bail!("embedding batch must not be empty");
        }
        if inputs
            .iter()
            .any(|text| text.trim().is_empty() || text.chars().count() > MAX_INPUT_CHARS)
        {
            bail!("embedding input must be non-empty and at most {MAX_INPUT_CHARS} characters");
        }

        let response = self
            .http
            .post(format!("{}/embed", self.base_url))
            .json(&EmbedRequest {
                inputs,
                truncate: true,
                normalize: self.normalize,
            })
            .send()
            .await
            .context("request TEI embeddings")?;
        let response = ensure_success_response(response, "embed").await?;
        let vectors: Vec<Vec<f32>> = response.json().await.context("decode TEI embeddings")?;
        validate_embedding_batch(&vectors, inputs.len())?;
        Ok(vectors)
    }
}

async fn ensure_success(response: reqwest::Response, operation: &str) -> Result<()> {
    ensure_success_response(response, operation)
        .await
        .map(|_| ())
}

async fn ensure_success_response(
    response: reqwest::Response,
    operation: &str,
) -> Result<reqwest::Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }
    let body = response.text().await.unwrap_or_default();
    let body = body.chars().take(MAX_ERROR_BODY).collect::<String>();
    let status = status.as_u16();
    bail!("TEI {operation} failed with HTTP {status}: {body}");
}

pub fn validate_embedding_batch(vectors: &[Vec<f32>], expected: usize) -> Result<usize> {
    if vectors.len() != expected {
        bail!(
            "TEI returned {} vectors for {expected} inputs",
            vectors.len()
        );
    }
    let dimensions = vectors.first().map_or(0, Vec::len);
    if dimensions == 0 {
        bail!("TEI returned an empty embedding");
    }
    if vectors
        .iter()
        .any(|vector| vector.len() != dimensions || vector.iter().any(|value| !value.is_finite()))
    {
        bail!("TEI returned vectors with inconsistent dimensions or non-finite values");
    }
    Ok(dimensions)
}

pub fn vector_literal(vector: &[f32]) -> Result<String> {
    if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
        bail!("cannot serialize an empty or non-finite embedding");
    }
    Ok(format!(
        "[{}]",
        vector
            .iter()
            .map(|value| value.to_string())
            .collect::<Vec<_>>()
            .join(",")
    ))
}

pub fn content_hash(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_dimensions_and_values() {
        assert_eq!(
            validate_embedding_batch(&[vec![1.0, 2.0], vec![3.0, 4.0]], 2).unwrap(),
            2
        );
        assert!(validate_embedding_batch(&[vec![1.0], vec![2.0, 3.0]], 2).is_err());
        assert!(validate_embedding_batch(&[vec![f32::NAN]], 1).is_err());
    }

    #[test]
    fn serializes_pgvector_literal() {
        assert_eq!(vector_literal(&[1.0, -0.5]).unwrap(), "[1,-0.5]");
        assert!(vector_literal(&[f32::INFINITY]).is_err());
    }

    #[test]
    fn hashes_utf8_text_deterministically() {
        assert_eq!(
            content_hash("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn rejects_empty_url() {
        assert!(TeiClient::new("///", None, "test", Duration::from_secs(1), true).is_err());
    }
}
