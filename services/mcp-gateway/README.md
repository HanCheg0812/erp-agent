# Business MCP

Rust stdio MCP gateway, launched as two servers (`erp` and `openviking`) by JCode's native McpManager. Build with `npm run mcp:build`. Runtime currently runs on the host alongside the bridge; sandbox placement remains separate acceptance work.

Published tools: `list_erp_orders`, `read_erp_order`, `search_case_knowledge`, `read_knowledge_source`. Requests use the private scoped session token; ERP/OpenViking credentials and data grants remain in the Rust API. No writes are published. ERPNext list/read passed real model tests. OpenViking tools return an explicit unconfigured error until its service and embedding are configured.

See [integration evidence](../../docs/plantree/plans/server-demo/mcp-composer-integration.md) and [approval research](../../docs/plantree/plans/server-demo/approval-modes-research.md). Agent approval modes are not yet implemented; data authorization remains enforced by the API.
