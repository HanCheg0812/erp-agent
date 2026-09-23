use anyhow::{Result, ensure};
use async_trait::async_trait;
use jcode_app_core::tool::{Tool, ToolContext, ToolOutput, ToolSuspended};
use serde_json::{Value, json};
pub const TOOLS: [&str; 4] = [
    "memory_save",
    "case_set_context",
    "proposal_create",
    "request_proposal_approval",
];
pub struct AuthorizedAction(pub &'static str);

fn object(properties: Value) -> Value {
    let required: Vec<_> = properties.as_object().unwrap().keys().cloned().collect();
    json!({"type":"object","properties":properties,"required":required,"additionalProperties":false})
}

pub fn schema(name: &str) -> Value {
    let string = json!({"type":"string","minLength":1});
    let version = json!({"type":"integer","minimum":1});
    match name {
        "memory_save" => object(json!({"text":{"type":"string","minLength":1,"maxLength":2000}})),
        "case_set_context" => object(
            json!({"expected_case_version":version,"context":object(json!({
                "title":string,"customer_code":string,"part_number":string,"revision":string,
                "demand_type":{"type":"string","enum":["forecast","order","delivery_instruction"]},
                "quantity_basis":{"type":"string","enum":["incremental","cumulative","replacement","unknown"]}
            }))}),
        ),
        "proposal_create" => object(json!({"expected_case_version":version,"summary":string,
            "actions":{"type":"array","minItems":1,"maxItems":100,"items":object(json!({
                "type":{"type":"string","enum":["revise_sales_order_item"]},"sales_order":string,"item_row":string,"expected_erp_modified":string,
                "expected_quantity":{"type":"integer","minimum":0},"proposed_quantity":{"type":"integer","minimum":0},
                "expected_delivery_date":{"type":"string","description":"YYYY-MM-DD"},"proposed_delivery_date":{"type":"string","description":"YYYY-MM-DD"}
            }))},
            "evidence":{"type":"array","minItems":1,"maxItems":100,"items":object(json!({
                "document_id":{"type":"string","description":"ID of an actual document attached to the current case, returned by case_read. Never invent an ID."},"revision":string,"locator":string
            }))}
        })),
        "request_proposal_approval" => object(
            json!({"proposal_id":string,"proposal_digest":string,"expected_case_version":version}),
        ),
        _ => unreachable!(),
    }
}

pub async fn request(method: reqwest::Method, path: &str, body: Option<Value>) -> Result<Value> {
    let url = format!(
        "{}/internal/agent-sessions/{}/{}",
        std::env::var("JCODE_SESSION_STORE_URL")?,
        std::env::var("JCODE_SESSION_STORE_ID")?,
        path
    );
    let mut request = reqwest::Client::new()
        .request(method, url)
        .bearer_auth(std::env::var("JCODE_SESSION_STORE_TOKEN")?)
        .timeout(std::time::Duration::from_secs(if path == "attachments/read" { 90 } else { 10 }));
    if let Some(body) = body {
        request = request.json(&body);
    }
    let response = request.send().await?;
    let status = response.status();
    let body = response.text().await?;
    let value: Value = serde_json::from_str(&body).map_err(|error| {
        anyhow::anyhow!("Action response was not JSON (HTTP {status}): {error}")
    })?;
    ensure!(
        status.is_success(),
        "Action rejected: {}",
        value["error"].as_str().unwrap_or("request_failed")
    );
    Ok(value)
}

#[async_trait]
impl Tool for AuthorizedAction {
    fn name(&self) -> &str {
        self.0
    }
    fn description(&self) -> &str {
        match self.0 {
            "memory_save" => {
                "Save an explicitly requested personal preference or working convention for the current user. Never store credentials or approval authority. Backend handles operation approval."
            }
            "case_set_context" => {
                "Add confirmed business context to the current plain conversation. Read case_read first for version; use facts supplied by the user or verified sources, never invent missing details. Backend handles operation approval. Conversation stays in the same session."
            }
            "proposal_create" => {
                "Save an exact-version order change proposal with source evidence in the current case. Read case_read and the ERP order first; quantity basis must be replacement and evidence IDs must belong to this case. Backend handles operation approval. This neither signs business approval nor writes to ERP. Use request_proposal_approval after saving to present the human business decision."
            }
            "request_proposal_approval" => {
                "Present the saved proposal to an authorized human for business approval/rejection and suspend this tool call until a decision. Use exact proposal_id, digest and case_version returned by proposal_create or case_read. Available in every operation approval mode, including full. Chat consent and automatic operation review cannot sign business approval. Returns the recorded decision on resume; never executes ERP changes."
            }
            _ => unreachable!(),
        }
    }
    fn parameters_schema(&self) -> Value {
        schema(self.0)
    }
    async fn execute(&self, mut input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        if let Some(map) = input.as_object_mut() {
            map.remove("intent");
            map.remove("accept_large_output");
        }
        let value = request(
            reqwest::Method::POST,
            "actions",
            Some(json!({"tool_call_id":ctx.tool_call_id,"tool":self.0,"arguments":input})),
        )
        .await?;
        if value["status"] == "pending" {
            return Err(ToolSuspended.into());
        }
        Ok(ToolOutput::new(value.to_string()))
    }
}
