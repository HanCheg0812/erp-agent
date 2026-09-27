use anyhow::{Result, ensure};
use async_trait::async_trait;
use jcode_app_core::tool::{Tool, ToolContext, ToolOutput};
use serde_json::{Value, json};

pub struct CubeBrowserBrowse;

#[async_trait]
impl Tool for CubeBrowserBrowse {
    fn name(&self) -> &str {
        "cube_browser_browse"
    }

    fn description(&self) -> &str {
        "Open one HTTPS page in a separately isolated Cube browser environment and return a bounded title, heading and text excerpt. The backend enforces the host allowlist; this tool cannot submit forms, run code, access credentials or change ERP records."
    }

    fn parameters_schema(&self) -> Value {
        json!({"type":"object","properties":{"url":{"type":"string","format":"uri","minLength":12,"maxLength":2048}},"required":["url"],"additionalProperties":false})
    }

    async fn execute(&self, mut input: Value, ctx: ToolContext) -> Result<ToolOutput> {
        if let Some(map) = input.as_object_mut() {
            map.remove("intent");
            map.remove("accept_large_output");
        }
        ensure!(input["url"].is_string(), "Browser URL is required");
        let result = super::actions::request(
            reqwest::Method::POST,
            "cube/browser-browse",
            Some(json!({"tool_call_id":ctx.tool_call_id,"url":input["url"]})),
        )
        .await?;
        Ok(ToolOutput::new(result.to_string()))
    }
}
