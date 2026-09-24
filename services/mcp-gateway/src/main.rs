use rmcp::{ErrorData, RoleServer, ServerHandler, ServiceExt, model::*, service::RequestContext};
use serde_json::{Value, json};

struct Gateway {
    domain: String,
    endpoint: String,
    token: String,
    http: reqwest::Client,
}
impl Gateway {
    fn tools(&self) -> Vec<Tool> {
        let specs = if self.domain == "erp" {
            vec![
                (
                    "list_erp_orders",
                    "List current ERPNext sales orders within this user's backend-granted customer scope. No writes. Success returns status, as_of, trace_id, sources, data.orders and warnings. Permission failures include agent_guidance; follow it and do not retry with altered arguments.",
                    json!({"type":"object","properties":{"limit":{"type":"integer","minimum":1,"maximum":20}},"additionalProperties":false}),
                ),
                (
                    "read_erp_order",
                    "Read one current ERPNext sales order and line items. The backend enforces customer and case scope. No writes. Success returns status, as_of, trace_id, sources, data.order and warnings. Permission failures include agent_guidance; follow it and do not infer hidden records.",
                    json!({"type":"object","properties":{"sales_order":{"type":"string","minLength":1,"maxLength":140}},"required":["sales_order"],"additionalProperties":false}),
                ),
            ]
        } else {
            vec![
                (
                    "search_case_knowledge",
                    "Search OpenViking evidence within this user's authorized case roots. Returns source URIs. If the service is unconfigured, report that result; never invent evidence.",
                    json!({"type":"object","properties":{"query":{"type":"string","minLength":1,"maxLength":1000},"limit":{"type":"integer","minimum":1,"maximum":10}},"required":["query"],"additionalProperties":false}),
                ),
                (
                    "read_knowledge_source",
                    "Read an OpenViking source returned by scoped search. Backend validates the source URI against authorized case roots.",
                    json!({"type":"object","properties":{"uri":{"type":"string","minLength":1,"maxLength":1000}},"required":["uri"],"additionalProperties":false}),
                ),
            ]
        };
        specs
            .into_iter()
            .map(|(name, description, schema)| {
                let mut tool = Tool::new(name, description, schema.as_object().unwrap().clone());
                tool.annotations = Some(ToolAnnotations::new().read_only(true).destructive(false));
                tool
            })
            .collect()
    }
}
impl ServerHandler for Gateway {
    fn get_info(&self) -> ServerConfig {
        let mut info = ServerConfig::default();
        info.capabilities = ServerCapabilities::builder().enable_tools().build();
        info
    }
    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        Ok(ListToolsResult::with_all_items(self.tools()))
    }
    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        if !self.tools().iter().any(|t| t.name == request.name) {
            return Err(ErrorData::invalid_params("Unknown domain tool", None));
        }
        let response = self.http.post(&self.endpoint).bearer_auth(&self.token)
            .json(&json!({"domain":self.domain,"tool":request.name,"arguments":request.arguments.unwrap_or_default()}))
            .send().await;
        let result = match response {
            Ok(response) => {
                let ok = response.status().is_success();
                let body = response
                    .json::<Value>()
                    .await
                    .unwrap_or(json!({"error":"invalid_service_response"}));
                if ok {
                    CallToolResult::success(vec![ContentBlock::text(body.to_string())])
                } else {
                    CallToolResult::error(vec![ContentBlock::text(body.to_string())])
                }
            }
            Err(_) => CallToolResult::error(vec![ContentBlock::text(
                "Business service unavailable. No result was retrieved.",
            )]),
        };
        Ok(result.into())
    }
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let domain = std::env::args().nth(1).unwrap_or_default();
    anyhow::ensure!(
        ["erp", "openviking"].contains(&domain.as_str()),
        "Invalid domain"
    );
    let origin = std::env::var("JCODE_SESSION_STORE_URL")?;
    let session = std::env::var("JCODE_SESSION_STORE_ID")?;
    let gateway = Gateway {
        domain,
        endpoint: format!("{origin}/internal/agent-sessions/{session}/tools"),
        token: std::env::var("JCODE_SESSION_STORE_TOKEN")?,
        http: reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(20))
            .build()?,
    };
    gateway
        .serve(rmcp::transport::stdio())
        .await?
        .waiting()
        .await?;
    Ok(())
}
