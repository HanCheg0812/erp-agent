use super::types::{Action, Decision, Memory, ProposalRequest};
use crate::{cases, platform::*};
use serde_json::Value;

pub(super) fn validate_action(input: &Action) -> Result<()> {
    text(&input.tool_call_id, 256)?;
    match input.tool.as_str() {
        "memory_save" => {
            let memory: Memory = parse(input.arguments.clone())?;
            text(&memory.text, 2000)?;
        }
        "case_set_context" => cases::validate_context(input.arguments.clone())?,
        "proposal_create" => cases::validate_proposal(input.arguments.clone())?,
        "request_proposal_approval" => {
            let _: ProposalRequest = parse(input.arguments.clone())?;
        }
        _ => return Err(Error::bad("Unknown action")),
    }
    Ok(())
}
pub(super) fn permission_mode<'a>(tool: &str, configured: &'a str) -> &'a str {
    if tool == "request_proposal_approval" {
        "ask"
    } else {
        configured
    }
}
pub(super) fn check_proposal_request(
    current: &Value,
    request: &ProposalRequest,
    matches: bool,
) -> Result<()> {
    if !matches
        || current["version"] != request.expected_case_version
        || current["status"] != "awaiting_approval"
    {
        return Err(Error::conflict("stale_proposal"));
    }
    Ok(())
}
pub(super) fn check_call_binding(
    permission: &Value,
    input: &Action,
    fingerprint: &str,
    mid: i64,
) -> Result<()> {
    if permission["arguments_digest"] != fingerprint
        || permission["tool"] != input.tool
        || permission["message_id"] != mid
    {
        return Err(Error::conflict("approval_arguments_changed"));
    }
    Ok(())
}
pub(super) fn should_expire(permission: &Value, expired: bool) -> bool {
    expired && ["pending", "allowed"].contains(&permission["state"].as_str().unwrap_or(""))
}
// true means a user's exact prior decision is being replayed, not a new write.
pub(super) fn decision_replay(permission: &Value, input: &Decision) -> Result<bool> {
    if permission["arguments_digest"] != input.arguments_digest || permission["mode"] != "ask" {
        return Err(Error::conflict("approval_changed"));
    }
    if permission["tool"] == "request_proposal_approval" {
        return Err(Error::bad(
            "Use the exact-version business approval endpoint",
        ));
    }
    if permission["state"] == "pending" {
        return Ok(false);
    }
    if permission["decided_by"] == "user"
        && (permission["state"] == input.decision
            || (input.decision == "allowed" && permission["state"] == "executed"))
    {
        return Ok(true);
    }
    Err(Error::conflict("approval_already_decided"))
}
pub(super) fn check_active_decision(state: &str, cancelled: bool, expired: bool) -> Result<()> {
    if expired || cancelled || !["running", "waiting"].contains(&state) {
        return Err(Error::conflict("approval_inactive"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn permission() -> Value {
        json!({"arguments_digest":"abc","mode":"ask","tool":"memory_save","state":"pending","decided_by":null,"message_id":42})
    }
    fn decision() -> Decision {
        Decision {
            arguments_digest: "abc".into(),
            decision: "allowed".into(),
        }
    }
    #[test]
    fn human_business_signature_cannot_be_granted_by_generic_modes() {
        for mode in ["ask", "auto", "full"] {
            assert_eq!(permission_mode("request_proposal_approval", mode), "ask");
            assert_eq!(permission_mode("memory_save", mode), mode);
        }
        let mut p = permission();
        p["tool"] = json!("request_proposal_approval");
        assert!(decision_replay(&p, &decision()).is_err());
    }
    #[test]
    fn replay_binds_digest_mode_and_original_decider() {
        assert!(!decision_replay(&permission(), &decision()).unwrap());
        for state in ["allowed", "executed"] {
            let mut p = permission();
            p["state"] = json!(state);
            p["decided_by"] = json!("user");
            assert!(decision_replay(&p, &decision()).unwrap());
            for (key, val) in [
                ("arguments_digest", "changed"),
                ("mode", "full"),
                ("decided_by", "reviewer"),
                ("state", "denied"),
            ] {
                let mut changed = p.clone();
                changed[key] = json!(val);
                assert!(decision_replay(&changed, &decision()).is_err(), "{key}");
            }
        }
    }
    #[test]
    fn execution_binds_call_and_message_and_only_live_permissions_expire() {
        let input = Action {
            tool_call_id: "call".into(),
            tool: "memory_save".into(),
            arguments: json!({"text":"hello"}),
        };
        assert!(check_call_binding(&permission(), &input, "abc", 42).is_ok());
        assert!(check_call_binding(&permission(), &input, "abc", 43).is_err());
        assert!(check_call_binding(&permission(), &input, "changed", 42).is_err());
        for state in ["pending", "allowed", "executed", "denied", "cancelled"] {
            let mut p = permission();
            p["state"] = json!(state);
            assert_eq!(
                should_expire(&p, true),
                ["pending", "allowed"].contains(&state)
            );
            assert!(!should_expire(&p, false));
        }
    }
    #[test]
    fn cancelled_expired_or_terminal_runs_cannot_accept_new_decisions() {
        for state in ["running", "waiting"] {
            assert!(check_active_decision(state, false, false).is_ok());
            assert!(check_active_decision(state, true, false).is_err());
            assert!(check_active_decision(state, false, true).is_err());
        }
        for state in ["queued", "completed", "failed", "cancelled"] {
            assert!(check_active_decision(state, false, false).is_err());
        }
    }
}
