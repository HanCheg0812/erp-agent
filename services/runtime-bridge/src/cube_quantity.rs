use anyhow::Result;
use async_trait::async_trait;
use jcode_app_core::tool::{Tool, ToolContext, ToolOutput};
use serde_json::{Value, json};

pub struct CubeQuantitySummary;

#[async_trait]
impl Tool for CubeQuantitySummary {
    fn name(&self) -> &str {
        "cube_quantity_summary"
    }

    fn description(&self) -> &str {
        "Summarize a short list of nonnegative manufacturing quantities in a separate, approved Cube code environment. Returns count, total, minimum and maximum. The backend owns the code and template; this does not read ERP or change business records."
    }

    fn parameters_schema(&self) -> Value {
        json!({"type":"object","properties":{"quantities":{"type":"array","minItems":1,
            "maxItems":32,"items":{"type":"integer","minimum":0,"maximum":1000000}}},
            "required":["quantities"],"additionalProperties":false})
    }

    async fn execute(&self, mut input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        if let Some(map) = input.as_object_mut() {
            map.remove("intent");
            map.remove("accept_large_output");
        }
        let result = super::actions::request(
            reqwest::Method::POST,
            "cube/quantity-summary",
            Some(json!({"tool_call_id":ctx.tool_call_id,"quantities":input["quantities"]})),
        )
        .await?;
        Ok(ToolOutput::new(result.to_string()))
    }
}
