# Cube Agent template

This directory defines the first Cube-facing runtime contract for the Forge
Agent.  The template process is a resident Rust HTTP service.  It exposes:

- `GET /health/live` for process liveness;
- `GET /health/ready` for the Cube application probe;
- `POST /v1/turn` for one authorized session turn.

Set `AGENT_RUNTIME_MODE=resident` when the guest starts the runtime bridge. The
default mode remains the existing private stdin/stdout worker path, so this
entry point is reversible while the resident JCode refactor is being verified.

## Warmup contract

The image build uses `CUBE_TEMPLATE_WARMUP=1`. In this mode the process checks
only its executable and HTTP listener; no model key, session token, MCP token or
customer data is embedded in the image. After creating a normal v0.7.2
instance, the backend sends its `OPENAI_API_KEY`, `JCODE_OPENAI_MODEL`, PostgreSQL
session store values, `AGENT_MCP_BINARY` (the image's
`/usr/local/bin/erp-agent-mcp`) and runtime token through the authenticated
guest `/v1/configure` endpoint. Instance creation does not rely on envd.
Clients must send that value in `x-agent-runtime-token`. `/v1/turn` initializes a
resident JCode Agent on the first request and returns the same NDJSON event contract
on every request.

`/health/ready` reports `resident_agent: true` and exposes whether the Agent has
already been initialized. With `AGENT_EXECUTION_BACKEND=cube`, the business
Worker provisions a case instance on the first queued turn and reuses it for
later turns. The default backend remains the existing local process path. Each
Cube instance is bound to one PostgreSQL/JCode session; a second active request gets
`409 session_busy`, and a request attempting `new_session` after initialization
gets `409 session_bound`.

## Cube probe

The template's application probe must target port `49999` and path
`/health/ready`. Do not probe only envd or the TCP listener. Do not put provider
keys in a template artifact; pass them through the per-instance runtime
configuration and rotate them outside the sandbox.

## Image build

Build the Rust binary first, then build this image from the repository root:

```sh
cargo build --release --manifest-path services/runtime-bridge/Cargo.toml
docker build \
  --build-arg RUNTIME_BINARY=services/runtime-bridge/target/release/erp-agent-runtime \
  --build-arg MCP_BINARY=services/mcp-gateway/target/release/erp-agent-mcp \
  -f infra/cube/agent-template/Dockerfile \
  -t forge-agent-runtime:resident-local .
```

The image uses Ubuntu 24.04 because the current Rust toolchain emits a binary
requiring GLIBC 2.39. If the Cube node is pinned to an older rootfs, build the
binary inside that same rootfs or publish a musl-compatible build instead of
silently accepting a container that exits before readiness.

The Cube template creation request should set `CUBE_TEMPLATE_WARMUP=1`, expose
guest port `49999`, and probe `/health/ready`. The warmup environment remains
credential-free. The backend configures each normal instance after creation,
before the first turn. The Bridge emits its skill catalog on every turn because
the Worker validates the catalog separately for each persisted run.
