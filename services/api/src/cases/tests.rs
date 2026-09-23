use super::{domain::*, types::*};
use crate::platform::digest;
use serde_json::{Value, json};
use uuid::Uuid;

fn context() -> Value {
    json!({"expected_case_version":1,"context":{"title":" Order change ","customer_code":" C1 ","part_number":" P1 ","revision":" A ","demand_type":"order","quantity_basis":"replacement"}})
}
fn proposal() -> Value {
    json!({"expected_case_version":1,"summary":"  Keep revision A  ","actions":[{"type":"revise_sales_order_item","sales_order":"SO-1","item_row":"10","expected_erp_modified":"2026-09-22T00:00:00Z","expected_quantity":1,"proposed_quantity":2,"expected_delivery_date":"2028-02-29","proposed_delivery_date":"2028-03-01"}],"evidence":[{"document_id":Uuid::nil(),"revision":"A","locator":"page 1"}]})
}
fn approval() -> Value {
    json!({"proposal_id":Uuid::nil(),"proposal_digest":"a".repeat(64),"expected_case_version":2,"decision":"approved","comment":"  Confirmed  "})
}

#[test]
fn context_transition_is_normalized_and_idempotent_only_for_exact_fields() {
    let input = context_input(context()).unwrap();
    assert_eq!(input.context.title, "Order change");
    assert_eq!(
        context_transition(&json!({"kind":"conversation","version":1}), &input).unwrap(),
        Some(2)
    );
    let mut saved = json!(input.context);
    saved["kind"] = json!("case");
    saved["version"] = json!(2);
    assert_eq!(context_transition(&saved, &input).unwrap(), None);
    saved["revision"] = json!("B");
    assert_eq!(
        context_transition(&saved, &input).unwrap_err().1,
        "stale_case"
    );
    assert!(next_version(i32::MAX).is_err());
    assert!(
        context_input(json!({"expected_case_version":0,"context":context()["context"]})).is_err()
    );
}

#[test]
fn proposal_validation_keeps_normalized_value_and_rejects_invalid_actions() {
    assert_eq!(
        proposal_input(proposal()).unwrap().summary,
        "Keep revision A"
    );
    for (field, value) in [
        ("type", json!("arbitrary_write")),
        ("expected_quantity", json!(-1)),
        ("proposed_quantity", json!(1_000_000_001_i64)),
        ("expected_delivery_date", json!("2027-02-29")),
        ("proposed_delivery_date", json!("2028-2-29")),
    ] {
        let mut input = proposal();
        input["actions"][0][field] = value;
        assert!(proposal_input(input).is_err(), "{field}");
    }
    let mut input = proposal();
    input["evidence"] = json!([]);
    assert!(proposal_input(input).is_err());
    let mut input = proposal();
    input["extra"] = json!(true);
    assert!(proposal_input(input).is_err());
}

#[test]
fn proposal_requires_context_exact_version_resolved_quantity_and_unapproved_state() {
    let valid = json!({"kind":"case","version":1,"quantity_basis":"replacement","status":"draft"});
    assert!(can_propose(&valid, 1).is_ok());
    for (field, value, code) in [
        ("kind", json!("conversation"), "case_context_required"),
        ("version", json!(2), "stale_case"),
        (
            "quantity_basis",
            json!("unknown"),
            "quantity_basis_unresolved",
        ),
        ("status", json!("approved"), "already_approved"),
    ] {
        let mut current = valid.clone();
        current[field] = value;
        assert_eq!(can_propose(&current, 1).unwrap_err().1, code);
    }
}

#[test]
fn approval_validation_and_legacy_digest_are_stable() {
    let input = approval_input(approval()).unwrap();
    assert_eq!(input.comment, "Confirmed");
    let expected = format!(
        r#"{{"caseId":"00000000-0000-0000-0000-000000000000","proposal_id":"00000000-0000-0000-0000-000000000000","proposal_digest":"{}","expected_case_version":2,"decision":"approved","comment":"Confirmed"}}"#,
        "a".repeat(64)
    );
    assert_eq!(approval_digest(Uuid::nil(), &input), digest(expected));
    assert!(can_approve(&json!({"version":2,"status":"awaiting_approval"}), &input).is_ok());
    assert!(can_approve(&json!({"version":3,"status":"awaiting_approval"}), &input).is_err());
    assert!(can_approve(&json!({"version":2,"status":"approved"}), &input).is_err());
    for key in [None, Some("short"), Some("has space"), Some("bad/key/123")] {
        assert!(approval_key(key).is_err());
    }
    assert!(approval_key(Some("request_123-456")).is_ok());
    let mut value = approval();
    value["proposal_digest"] = json!("A".repeat(64));
    assert!(approval_input(value).is_err());
    let mut value = approval();
    value["decision"] = json!("execute_erp");
    assert!(approval_input(value).is_err());
}

#[test]
fn pagination_limits_are_bounded() {
    assert_eq!(Page::default().limit().unwrap(), 50);
    assert!(
        Page {
            limit: Some(101),
            ..Default::default()
        }
        .limit()
        .is_err()
    );
    assert!(
        Page {
            limit: Some(0),
            ..Default::default()
        }
        .limit()
        .is_err()
    );
}

#[test]
fn case_adapters_and_use_cases_do_not_own_sql() {
    let handler = include_str!("handlers.rs");
    for forbidden in [
        "app.db",
        "transaction(",
        "actor.require",
        "SELECT ",
        "INSERT ",
        "UPDATE ",
    ] {
        assert!(
            !handler.contains(forbidden),
            "HTTP adapter contains {forbidden}"
        );
    }
    for source in [
        include_str!("application/approvals.rs"),
        include_str!("application/context.rs"),
        include_str!("application/proposals.rs"),
        include_str!("application/queries.rs"),
        include_str!("application/records.rs"),
    ] {
        for forbidden in [
            "SELECT ",
            "INSERT ",
            "UPDATE ",
            "DELETE ",
            "Json<",
            "HeaderMap",
            "State<",
        ] {
            assert!(!source.contains(forbidden), "Use case contains {forbidden}");
        }
    }
}
