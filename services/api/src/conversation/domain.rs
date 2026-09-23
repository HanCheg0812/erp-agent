use crate::platform::*;
use serde_json::Value;

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Operation {
    Message,
    Compact,
}
impl Operation {
    pub(super) fn as_str(self) -> &'static str {
        match self {
            Self::Message => "message",
            Self::Compact => "compact",
        }
    }
}
pub(super) fn check_replay(
    message: &Value,
    prior: &str,
    text: &str,
    settings: &Value,
    attachment_ids: &[uuid::Uuid],
    operation: Operation,
) -> Result<()> {
    let prior_ids: Vec<String> = message["attachments"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|item| item["id"].as_str().map(str::to_owned))
        .collect();
    let ids: Vec<String> = attachment_ids.iter().map(ToString::to_string).collect();
    if message["text"] != text
        || message["settings"] != *settings
        || prior_ids != ids
        || prior != operation.as_str()
    {
        return Err(Error::conflict("idempotency_conflict"));
    }
    Ok(())
}
pub(super) fn title(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(80)
        .collect()
}
pub(super) fn cancellation_transition(state: &str, requested: bool) -> Option<&'static str> {
    if requested {
        return None;
    }
    match state {
        // A running worker must persist its checkpoint before it is terminal.
        "running" => Some("running"),
        "queued" | "waiting" => Some("cancelled"),
        _ => None,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn retries_bind_text_settings_and_operation() {
        let settings = json!({"approval":"ask"});
        let message = json!({"text":"hello","settings":settings,"attachments":[]});
        assert!(
            check_replay(
                &message,
                "message",
                "hello",
                &settings,
                &[],
                Operation::Message
            )
            .is_ok()
        );
        assert!(
            check_replay(
                &message,
                "message",
                "changed",
                &settings,
                &[],
                Operation::Message
            )
            .is_err()
        );
        assert!(
            check_replay(
                &message,
                "message",
                "hello",
                &json!({"approval":"full"}),
                &[],
                Operation::Message
            )
            .is_err()
        );
        assert!(
            check_replay(
                &message,
                "message",
                "hello",
                &settings,
                &[],
                Operation::Compact
            )
            .is_err()
        );
        assert!(
            check_replay(
                &message,
                "message",
                "hello",
                &settings,
                &[uuid::Uuid::nil()],
                Operation::Message
            )
            .is_err()
        );
    }
    #[test]
    fn cancellation_waits_for_running_checkpoint_and_is_idempotent() {
        assert_eq!(cancellation_transition("running", false), Some("running"));
        for state in ["queued", "waiting"] {
            assert_eq!(cancellation_transition(state, false), Some("cancelled"));
        }
        for state in ["completed", "cancelled", "failed", "unknown"] {
            assert_eq!(cancellation_transition(state, false), None);
        }
        for state in ["running", "queued", "waiting"] {
            assert_eq!(cancellation_transition(state, true), None);
        }
    }
    #[test]
    fn title_normalizes_whitespace_and_bounds_unicode_characters() {
        assert_eq!(title("  hello\nworld  "), "hello world");
        assert_eq!(title(&"件".repeat(81)).chars().count(), 80);
    }
}
