use crate::platform::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub const TOOLS: [&str; 13] = [
    "memory_save",
    "case_set_context",
    "proposal_create",
    "request_proposal_approval",
    "case_read",
    "attachment_read",
    "todo",
    "skill_find",
    "skill_manage",
    "mcp__erp__list_erp_orders",
    "mcp__erp__read_erp_order",
    "mcp__openviking__search_case_knowledge",
    "mcp__openviking__read_knowledge_source",
];

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub approval: String,
    pub model: String,
    pub effort: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            approval: "ask".into(),
            model: env("DEEPSEEK_MODEL", "deepseek-flash"),
            effort: "high".into(),
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if !["ask", "auto", "full"].contains(&self.approval.as_str())
            || !["deepseek-flash", "deepseek-v4-pro"].contains(&self.model.as_str())
            || !["none", "low", "high", "max"].contains(&self.effort.as_str())
        {
            return Err(Error::bad("Unsupported agent settings"));
        }
        Ok(())
    }
}
pub fn config() -> Value {
    json!({"endpoint":env("DEEPSEEK_BASE_URL","https://api.deepseek.com"),"api":"responses",
        "models":["deepseek-flash","deepseek-v4-pro"],"efforts":["none","low","high","max"],
        "context_windows":{"deepseek-flash":1048576,"deepseek-v4-pro":1048576},
        "approval_modes":["ask","auto","full"],"integrations":{"erp":configured("ERP_API_TOKEN"),"openviking":configured("OPENVIKING_URL")}})
}
pub fn model_context_window(model: &str) -> Option<u64> {
    match model {
        "deepseek-flash" | "deepseek-v4-pro" => Some(1_048_576),
        _ => None,
    }
}

pub fn context_usage(model: &str, used_tokens: u64, compacting: bool) -> Value {
    let configured_soft = env("AGENT_COMPACTION_SOFT_RATIO", "0.80")
        .parse::<f64>()
        .unwrap_or(0.80);
    let configured_critical = env("AGENT_COMPACTION_CRITICAL_RATIO", "0.90")
        .parse::<f64>()
        .unwrap_or(0.90);
    let (soft_ratio, critical_ratio) = if configured_soft.is_finite()
        && configured_critical.is_finite()
        && 0.0 < configured_soft
        && configured_soft < configured_critical
        && configured_critical < 1.0
    {
        (configured_soft, configured_critical)
    } else {
        (0.80, 0.90)
    };
    json!({
        "model":model,
        "used_tokens":used_tokens,
        "window_tokens":model_context_window(model).unwrap_or(1_048_576),
        "soft_ratio":soft_ratio,
        "critical_ratio":critical_ratio,
        "compacting":compacting,
        "estimated":true
    })
}
fn configured(key: &str) -> bool {
    std::env::var(key).is_ok_and(|v| !v.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_accept_only_published_modes_models_and_efforts() {
        for approval in ["ask", "auto", "full"] {
            for model in ["deepseek-flash", "deepseek-v4-pro"] {
                for effort in ["none", "low", "high", "max"] {
                    assert!(
                        Settings {
                            approval: approval.into(),
                            model: model.into(),
                            effort: effort.into()
                        }
                        .validate()
                        .is_ok()
                    );
                }
            }
        }
        for settings in [
            Settings {
                approval: "invalid".into(),
                ..Default::default()
            },
            Settings {
                model: "unknown".into(),
                ..Default::default()
            },
            Settings {
                effort: "medium".into(),
                ..Default::default()
            },
        ] {
            assert!(settings.validate().is_err());
        }
        assert_eq!(TOOLS.len(), 13);
        assert!(TOOLS.contains(&"request_proposal_approval"));
        assert_eq!(model_context_window("deepseek-flash"), Some(1_048_576));
        assert_eq!(model_context_window("unknown"), None);
    }
}
