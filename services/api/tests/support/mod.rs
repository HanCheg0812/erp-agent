use deadpool_postgres::{Manager, Pool};
use erp_agent_api::App;
use std::time::Duration;
use tokio_postgres::NoTls;

pub fn app_without_live_dependencies() -> App {
    let mut config = tokio_postgres::Config::new();
    config
        .host("127.0.0.1")
        .port(1)
        .user("unavailable")
        .dbname("unavailable")
        .connect_timeout(Duration::from_millis(10));
    let db = Pool::builder(Manager::new(config, NoTls))
        .max_size(1)
        .runtime(deadpool_postgres::Runtime::Tokio1)
        .build()
        .unwrap();
    App {
        db,
        agent_wake: std::sync::Arc::new(tokio::sync::Notify::new()),
        host: "127.0.0.1".into(),
        port: 0,
        model_key: None,
        http: reqwest::Client::new(),
        passwords: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        attempts: Default::default(),
        bucket: None,
    }
}
