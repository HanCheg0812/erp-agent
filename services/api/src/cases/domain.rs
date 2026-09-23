use super::types::*;
use crate::platform::*;
use axum::http::StatusCode;
use serde::Serialize;
use serde_json::Value;
use uuid::Uuid;

pub(crate) fn validate_context(value: Value) -> Result<()> {
    context_input(value).map(|_| ())
}

pub(crate) fn validate_proposal(value: Value) -> Result<()> {
    proposal_input(value).map(|_| ())
}

pub(crate) fn context_input(value: Value) -> Result<ContextInput> {
    let input: ContextInput = parse(value)?;
    let context = input.context.validate()?;
    if input.expected_case_version < 1 {
        return Err(Error::bad("Invalid version"));
    }
    Ok(ContextInput {
        expected_case_version: input.expected_case_version,
        context,
    })
}

/// None means the exact context transition was already applied.
pub(crate) fn context_transition(current: &Value, input: &ContextInput) -> Result<Option<i32>> {
    let version = next_version(input.expected_case_version)?;
    let fields = serde_json::json!(input.context);
    if current["kind"] == "case"
        && current["version"] == version
        && fields
            .as_object()
            .unwrap()
            .iter()
            .all(|(key, value)| current[key] == *value)
    {
        return Ok(None);
    }
    if current["kind"] != "conversation" || current["version"] != input.expected_case_version {
        return Err(Error::conflict("stale_case"));
    }
    Ok(Some(version))
}

pub(crate) fn proposal_input(value: Value) -> Result<ProposalInput> {
    let mut input: ProposalInput = parse(value)?;
    input.summary = input.summary.trim().into();
    text(&input.summary, 4000)?;
    if input.expected_case_version < 1
        || !(1..=100).contains(&input.actions.len())
        || !(1..=100).contains(&input.evidence.len())
    {
        return Err(Error::bad("Invalid proposal"));
    }
    for action in &input.actions {
        text(&action.sales_order, 140)?;
        text(&action.item_row, 140)?;
        text(&action.expected_erp_modified, 100)?;
        if action.r#type != "revise_sales_order_item"
            || !(0..=1_000_000_000).contains(&action.expected_quantity)
            || !(0..=1_000_000_000).contains(&action.proposed_quantity)
            || !valid_date(&action.expected_delivery_date)
            || !valid_date(&action.proposed_delivery_date)
        {
            return Err(Error::bad("Invalid action"));
        }
    }
    for evidence in &input.evidence {
        text(&evidence.revision, 100)?;
        text(&evidence.locator, 500)?;
    }
    Ok(input)
}

pub(crate) fn can_propose(current: &Value, expected: i32) -> Result<()> {
    if current["kind"] == "conversation" {
        return Err(Error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "case_context_required",
            "Add case details before preparing a proposal",
        ));
    }
    if current["version"] != expected {
        return Err(Error::conflict("stale_case"));
    }
    if current["quantity_basis"] != "replacement" {
        return Err(Error(
            StatusCode::UNPROCESSABLE_ENTITY,
            "quantity_basis_unresolved",
            "Confirm replacement quantity first",
        ));
    }
    if current["status"] == "approved" {
        return Err(Error::conflict("already_approved"));
    }
    Ok(())
}

pub(crate) fn approval_input(value: Value) -> Result<ApprovalInput> {
    let mut input: ApprovalInput = parse(value)?;
    input.comment = input.comment.trim().into();
    if input.proposal_digest.len() != 64
        || !input
            .proposal_digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        || input.expected_case_version < 1
        || !["approved", "rejected"].contains(&input.decision.as_str())
        || input.comment.chars().count() > 2000
    {
        return Err(Error::bad("Invalid approval"));
    }
    Ok(input)
}

pub(crate) fn approval_key(key: Option<&str>) -> Result<&str> {
    key.filter(|key| {
        (8..=128).contains(&key.len())
            && key
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    })
    .ok_or_else(|| Error::bad("Idempotency key required"))
}

pub(crate) fn can_approve(current: &Value, input: &ApprovalInput) -> Result<()> {
    if current["version"] != input.expected_case_version || current["status"] != "awaiting_approval"
    {
        return Err(Error::conflict("stale_approval"));
    }
    Ok(())
}

pub(crate) fn approval_digest(id: Uuid, input: &ApprovalInput) -> String {
    // Saved approval retries depend on this serialized field order.
    #[derive(Serialize)]
    #[serde(rename_all = "camelCase")]
    struct Intent<'a> {
        case_id: Uuid,
        #[serde(flatten)]
        input: &'a ApprovalInput,
    }
    digest(serde_json::to_vec(&Intent { case_id: id, input }).unwrap())
}

pub(crate) fn next_version(version: i32) -> Result<i32> {
    version
        .checked_add(1)
        .ok_or_else(|| Error::conflict("version_limit"))
}
