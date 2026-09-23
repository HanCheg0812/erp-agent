use crate::platform::{Error, Result, parse, text};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaseInput {
    pub(crate) title: String,
    pub(crate) customer_code: String,
    pub(crate) part_number: String,
    pub(crate) revision: String,
    pub(crate) demand_type: String,
    pub(crate) quantity_basis: String,
}

impl CaseInput {
    pub(crate) fn validate(mut self) -> Result<Self> {
        self.title = self.title.trim().into();
        self.customer_code = self.customer_code.trim().into();
        self.part_number = self.part_number.trim().into();
        self.revision = self.revision.trim().into();
        text(&self.title, 200)?;
        for value in [&self.customer_code, &self.part_number, &self.revision] {
            text(value, 100)?;
        }
        if !["forecast", "order", "delivery_instruction"].contains(&self.demand_type.as_str())
            || !["incremental", "cumulative", "replacement", "unknown"]
                .contains(&self.quantity_basis.as_str())
        {
            return Err(Error::bad("Invalid demand or quantity basis"));
        }
        Ok(self)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ConversationInput {
    pub(crate) request_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ContextInput {
    pub(crate) expected_case_version: i32,
    pub(crate) context: CaseInput,
}

#[derive(Default, Deserialize)]
pub struct Page {
    pub limit: Option<i64>,
    pub after: Option<String>,
    pub before: Option<String>,
}

impl Page {
    pub fn limit(&self) -> Result<i64> {
        let n = self.limit.unwrap_or(50);
        if !(1..=100).contains(&n) {
            Err(Error::bad("Invalid page size"))
        } else {
            Ok(n)
        }
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Action {
    pub(crate) r#type: String,
    pub(crate) sales_order: String,
    pub(crate) item_row: String,
    pub(crate) expected_erp_modified: String,
    pub(crate) expected_quantity: i64,
    pub(crate) proposed_quantity: i64,
    pub(crate) expected_delivery_date: String,
    pub(crate) proposed_delivery_date: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Evidence {
    pub(crate) document_id: Uuid,
    pub(crate) revision: String,
    pub(crate) locator: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProposalInput {
    pub(crate) expected_case_version: i32,
    pub(crate) summary: String,
    pub(crate) actions: Vec<Action>,
    pub(crate) evidence: Vec<Evidence>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalInput {
    pub(crate) proposal_id: Uuid,
    pub(crate) proposal_digest: String,
    pub(crate) expected_case_version: i32,
    pub(crate) decision: String,
    #[serde(default)]
    pub(crate) comment: String,
}

pub(crate) fn parse_case(value: serde_json::Value) -> Result<CaseInput> {
    parse::<CaseInput>(value)?.validate()
}

pub(crate) fn valid_date(value: &str) -> bool {
    value.len() == 10
        && chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .is_ok_and(|date| date.format("%Y-%m-%d").to_string() == value)
}
