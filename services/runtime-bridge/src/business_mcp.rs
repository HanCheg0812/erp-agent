use anyhow::{Result, ensure};
use async_trait::async_trait;
use jcode_app_core::{
    mcp::{ContentBlock, McpConfig, McpManager, McpToolDef, dispatch_name},
    tool::{Tool, ToolContext, ToolOutput},
};
use serde_json::{Value, json};
use std::sync::Arc;

struct BusinessTool {
    server: String,
    definition: McpToolDef,
    manager: Arc<McpManager>,
}
#[async_trait]
impl Tool for BusinessTool {
    fn name(&self) -> &str {
        &self.definition.name
    }
    fn description(&self) -> &str {
        self.definition
            .description
            .as_deref()
            .unwrap_or("Scoped business read")
    }
    fn parameters_schema(&self) -> Value {
        self.definition.input_schema.clone()
    }
    fn mcp_identity(&self) -> Option<(&str, &str)> {
        Some((&self.server, &self.definition.name))
    }
    async fn execute(&self, mut input: Value, _: ToolContext) -> Result<ToolOutput> {
        if let Some(map) = input.as_object_mut() {
            map.remove("intent");
            map.remove("accept_large_output");
        }
        let result = self
            .manager
            .call_tool(&self.server, &self.definition.name, input)
            .await?;
        let text = result
            .content
            .into_iter()
            .filter_map(|c| match c {
                ContentBlock::Text { text } => Some(text),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        ensure!(!result.is_error, "{text}");
        Ok(ToolOutput::new(text))
    }
}
pub async fn connect() -> Result<Vec<(String, Arc<dyn Tool>)>> {
    let binary = std::env::var("AGENT_MCP_BINARY")?;
    let env = json!({
        "OPENAI_API_KEY":"",
        "JCODE_SESSION_STORE_URL":std::env::var("JCODE_SESSION_STORE_URL")?,
        "JCODE_SESSION_STORE_ID":std::env::var("JCODE_MCP_SESSION_ID").or_else(|_| std::env::var("JCODE_SESSION_STORE_ID"))?,
        "JCODE_SESSION_STORE_TOKEN":std::env::var("JCODE_MCP_SESSION_TOKEN").or_else(|_| std::env::var("JCODE_SESSION_STORE_TOKEN"))?,
    });
    let config: McpConfig = serde_json::from_value(json!({"servers":{
        "erp":{"command":binary,"args":["erp"],"shared":false,"env":env},
        "openviking":{"command":binary,"args":["openviking"],"shared":false,"env":env}
    }}))?;
    let manager = Arc::new(McpManager::with_config(config));
    let (count, errors) = manager.connect_all().await?;
    ensure!(
        count == 2 && errors.is_empty(),
        "Business MCP registration failed: {errors:?}"
    );
    let mut tools = Vec::new();
    for (server, definition) in manager.all_tools().await {
        let name = dispatch_name(&server, &definition.name);
        ensure!(
            [
                "mcp__erp__read_erp_order",
                "mcp__erp__list_erp_orders",
                "mcp__openviking__search_case_knowledge",
                "mcp__openviking__read_knowledge_source"
            ]
            .contains(&name.as_str()),
            "Unpublished MCP tool"
        );
        tools.push((
            name,
            Arc::new(BusinessTool {
                server,
                definition,
                manager: manager.clone(),
            }) as Arc<dyn Tool>,
        ));
    }
    ensure!(tools.len() == 4, "Missing business tools");
    Ok(tools)
}
