# Build the binary outside the image and pass it with:
#   docker build --build-arg RUNTIME_BINARY=services/runtime-bridge/target/release/erp-agent-runtime \
#     --build-arg MCP_BINARY=services/mcp-gateway/target/release/erp-agent-mcp .
FROM ubuntu:24.04

ARG RUNTIME_BINARY
ARG MCP_BINARY
COPY ${RUNTIME_BINARY} /usr/local/bin/erp-agent-runtime
COPY ${MCP_BINARY} /usr/local/bin/erp-agent-mcp
COPY infra/cube/agent-template/resident-start.sh /usr/local/bin/resident-start.sh

RUN chmod 0755 /usr/local/bin/erp-agent-runtime /usr/local/bin/erp-agent-mcp /usr/local/bin/resident-start.sh \
    && useradd --create-home --uid 10001 --shell /usr/sbin/nologin forge

USER forge
WORKDIR /home/forge/work
ENV AGENT_RUNTIME_MODE=resident \
    AGENT_RUNTIME_BIND=0.0.0.0:49999 \
    HOME=/home/forge \
    JCODE_HOME=/home/forge/jcode

EXPOSE 49999
ENTRYPOINT ["/usr/local/bin/resident-start.sh"]
