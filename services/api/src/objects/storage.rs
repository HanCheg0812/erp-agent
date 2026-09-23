use crate::platform::*;
use axum::http::StatusCode;
use s3::{Bucket, BucketConfiguration, Region, creds::Credentials};
use std::{sync::Arc, time::Duration};
fn credentials() -> anyhow::Result<Credentials> {
    Ok(Credentials::new(
        Some(&std::env::var("RUSTFS_ACCESS_KEY")?),
        Some(&std::env::var("RUSTFS_SECRET_KEY")?),
        None,
        None,
        None,
    )?)
}
fn region() -> Region {
    Region::Custom {
        region: env("RUSTFS_REGION", "us-east-1"),
        endpoint: env("RUSTFS_ENDPOINT", "http://127.0.0.1:9020"),
    }
}
pub fn bucket_from_env() -> anyhow::Result<Option<Arc<Bucket>>> {
    if std::env::var("RUSTFS_SECRET_KEY").is_err() {
        return Ok(None);
    }
    let mut bucket = Bucket::new(
        &env("RUSTFS_BUCKET", "erp-agent-documents"),
        region(),
        credentials()?,
    )?
    .with_path_style();
    bucket.set_request_timeout(Some(Duration::from_secs(15)));
    Ok(Some(Arc::from(bucket)))
}
pub async fn initialize(app: &App) -> anyhow::Result<()> {
    let bucket = app
        .bucket
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("RustFS credentials missing"))?;
    // List first so an existing private bucket is preserved without changing its policy.
    if bucket.list("".into(), None).await.is_err() {
        let result = Bucket::create_with_path_style(
            &bucket.name,
            region(),
            credentials()?,
            BucketConfiguration::default(),
        )
        .await?;
        anyhow::ensure!(
            (200..300).contains(&result.response_code),
            "Bucket creation failed: {}",
            result.response_code
        );
    }
    bucket.list("".into(), None).await?;
    Ok(())
}
pub(super) fn unavailable() -> Error {
    Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "storage_unavailable",
        "Document storage is unavailable",
    )
}

pub(super) async fn put(bucket: &Bucket, key: &str, data: &[u8], content_type: &str) -> Result<()> {
    let response = bucket
        .put_object_with_content_type(key, data, content_type)
        .await
        .map_err(|_| unavailable())?;
    if !(200..300).contains(&response.status_code()) {
        return Err(unavailable());
    }
    Ok(())
}
pub(crate) async fn get_verified(bucket: &Bucket, key: &str, hash: &str) -> Result<Vec<u8>> {
    let response = bucket.get_object(key).await.map_err(|_| unavailable())?;
    if response.status_code() != 200 || digest(response.as_slice()) != hash {
        return Err(unavailable());
    }
    Ok(response.to_vec())
}
