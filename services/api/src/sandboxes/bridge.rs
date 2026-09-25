use crate::platform::*;
use hmac::{Hmac, Mac};
use sha2::Sha256;
use uuid::Uuid;

type RuntimeMac = Hmac<Sha256>;

pub(crate) fn enabled() -> bool {
    env("AGENT_EXECUTION_BACKEND", "local") == "cube"
}

pub(crate) fn configured() -> bool {
    enabled()
        && shared_secret().is_ok()
        && port().is_ok()
        && [
            "CUBE_API_URL",
            "CUBE_PROXY_URL",
            "CUBE_TEMPLATE_ID",
            "AGENT_STORAGE_ORIGIN",
        ]
        .iter()
        .all(|name| std::env::var(name).is_ok_and(|value| !value.trim().is_empty()))
}

pub(crate) fn port() -> Result<u16> {
    let value = env("CUBE_RUNTIME_PORT", "49999")
        .parse::<u16>()
        .map_err(|_| Error::bad("Invalid Cube runtime port"))?;
    if !(1024..=65535).contains(&value) {
        return Err(Error::bad("Invalid Cube runtime port"));
    }
    Ok(value)
}

pub(super) fn shared_secret() -> Result<String> {
    let value = std::env::var("CUBE_RUNTIME_SHARED_SECRET").map_err(|_| {
        Error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "Cube runtime secret is not configured",
        )
    })?;
    if value.trim().len() < 32 {
        return Err(Error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "Cube runtime secret is too short",
        ));
    }
    Ok(value)
}

pub(super) fn runtime_token(secret: &str, operation: Uuid) -> String {
    let mut mac = RuntimeMac::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key");
    mac.update(b"erp-agent/cube-runtime/v1/");
    mac.update(operation.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) fn token_for(operation: Uuid) -> Result<String> {
    Ok(runtime_token(&shared_secret()?, operation))
}

pub(crate) fn guest_url(sandbox_id: &str, route: &str) -> Result<String> {
    if sandbox_id.is_empty()
        || sandbox_id.len() >= 128
        || !sandbox_id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        return Err(Error::bad("Invalid Cube sandbox identifier"));
    }
    if !route.starts_with('/') || route.contains("..") {
        return Err(Error::bad("Invalid Cube guest route"));
    }
    let proxy = std::env::var("CUBE_PROXY_URL").map_err(|_| {
        Error(
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            "cube_runtime_not_configured",
            "Cube proxy is not configured",
        )
    })?;
    Ok(format!(
        "{}/sandbox/{}/{port}{route}",
        proxy.trim_end_matches('/'),
        sandbox_id,
        port = port()?,
        route = route
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_token_is_stable_and_scoped_to_operation() {
        let operation = Uuid::from_u128(7);
        let first = runtime_token("a secret with at least 32 characters", operation);
        assert_eq!(first.len(), 64);
        assert_eq!(
            first,
            runtime_token("a secret with at least 32 characters", operation)
        );
        assert_ne!(
            first,
            runtime_token("a secret with at least 32 characters", Uuid::from_u128(8))
        );
        assert_ne!(
            first,
            runtime_token("a different secret with at least 32 chars", operation)
        );
    }

    #[test]
    fn guest_routes_reject_path_traversal_and_bad_ids() {
        assert!(guest_url("cube_1", "/health/ready").is_err());
        assert!(guest_url("../bad", "/health/ready").is_err());
    }
}
