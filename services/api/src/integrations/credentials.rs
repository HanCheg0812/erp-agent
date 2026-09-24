use crate::platform::*;
use axum::http::StatusCode;

pub(crate) trait ErpCredentialProvider: Send + Sync {
    fn resolve(&self, reference: Option<&str>) -> Result<String>;
}

#[derive(Clone, Copy, Default)]
pub(crate) struct EnvironmentCredentialProvider;

impl ErpCredentialProvider for EnvironmentCredentialProvider {
    fn resolve(&self, reference: Option<&str>) -> Result<String> {
        let name = reference
            .and_then(|value| value.strip_prefix("env:"))
            .filter(|value| {
                value.starts_with("ERP_CREDENTIAL_")
                    && value.len() > "ERP_CREDENTIAL_".len()
                    && value.len() <= 120
            })
            .ok_or_else(unavailable)?;
        if !name
            .bytes()
            .all(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || byte == b'_')
        {
            return Err(unavailable());
        }
        std::env::var(name)
            .ok()
            .filter(|value| !value.is_empty())
            .ok_or_else(unavailable)
    }
}

fn unavailable() -> Error {
    Error(
        StatusCode::SERVICE_UNAVAILABLE,
        "erp_credentials_unavailable",
        "The configured ERP credential is unavailable",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_provider_only_accepts_restricted_references() {
        let provider = EnvironmentCredentialProvider;
        for reference in [
            None,
            Some("env:ERP_API_TOKEN"),
            Some("env:ERP_CREDENTIAL_bad"),
            Some("vault:erp/forge-reader"),
        ] {
            assert!(provider.resolve(reference).is_err());
        }
    }
}
