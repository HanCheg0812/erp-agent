#[tokio::main]
async fn main() -> anyhow::Result<()> {
    erp_agent_api::run().await
}
