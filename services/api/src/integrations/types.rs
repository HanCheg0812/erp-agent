use serde::Deserialize;
use serde_json::Value;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Call {
    pub(super) domain: String,
    pub(super) tool: String,
    pub(super) arguments: Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn call_schema_rejects_unknown_or_missing_fields() {
        assert!(
            serde_json::from_value::<Call>(
                json!({"domain":"erp","tool":"list_erp_orders","arguments":{}})
            )
            .is_ok()
        );
        assert!(
            serde_json::from_value::<Call>(json!({"domain":"erp","tool":"list_erp_orders"}))
                .is_err()
        );
        assert!(
            serde_json::from_value::<Call>(
                json!({"domain":"erp","tool":"list_erp_orders","arguments":{},"extra":true})
            )
            .is_err()
        );
    }
}
