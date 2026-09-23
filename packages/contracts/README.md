# Shared Contracts

`src/index.ts` implements authentication, case/proposal/approval and pagination schemas
using Zod. Execution, agent session persistence, MCP and ingestion contracts remain to
be added with their corresponding implementations.

Define state transitions and versioning before wiring endpoints. Business approvals are distinct from JCode tool permission prompts.
