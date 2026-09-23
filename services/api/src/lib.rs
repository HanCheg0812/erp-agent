mod agent_sessions;
mod auth;
mod cases;
mod conversation;
mod integrations;
mod objects;
mod permissions;
mod platform;
mod sandboxes;
mod worker;

#[cfg(test)]
mod architecture_tests;

use axum::{
    Router,
    extract::DefaultBodyLimit,
    middleware,
    routing::{get, post},
};
pub use platform::App;
use platform::Result;
use std::sync::Arc;

pub fn router(app: App) -> Router {
    let protected = Router::new()
        .route("/auth/me", get(auth::me))
        .route("/auth/logout", post(auth::logout))
        .route("/conversations", post(cases::start_conversation))
        .route("/cases", get(cases::list).post(cases::create))
        .route("/cases/{id}", get(cases::detail))
        .route(
            "/cases/{id}/context",
            axum::routing::patch(cases::set_context),
        )
        .route("/cases/{id}/proposals", post(cases::propose))
        .route("/cases/{id}/approvals", post(cases::approve))
        .route("/cases/{id}/events", get(cases::events))
        .route("/cases/{id}/conversation", get(conversation::history))
        .route(
            "/cases/{id}/conversation/settings",
            axum::routing::patch(conversation::update_settings),
        )
        .route(
            "/cases/{id}/conversation/messages",
            post(conversation::send),
        )
        .route("/cases/{id}/compactions", post(conversation::compact))
        .route(
            "/cases/{id}/tool-permissions/{permission}",
            post(permissions::decide),
        )
        .route(
            "/cases/{id}/conversation/cancel",
            post(conversation::cancel),
        )
        .route("/cases/{id}/agent-sessions", post(agent_sessions::create))
        .route(
            "/agent-sessions/{id}/claim",
            post(agent_sessions::claim_route),
        )
        .route(
            "/cases/{id}/sandbox",
            get(sandboxes::status).post(sandboxes::create),
        )
        .route(
            "/cases/{id}/sandbox/{operation}",
            axum::routing::delete(sandboxes::release),
        )
        .route(
            "/cases/{id}/documents",
            get(objects::list).post(objects::upload),
        )
        .route("/documents/{id}", get(objects::download))
        .route(
            "/memory",
            get(objects::memory_list).post(objects::memory_save),
        )
        .route(
            "/memory/{id}",
            axum::routing::delete(objects::memory_delete),
        )
        .layer(middleware::from_fn_with_state(
            app.clone(),
            auth::authenticate,
        ));
    Router::new()
        .route(
            "/internal/agent-sessions/{id}/case",
            get(permissions::read_case),
        )
        .route(
            "/internal/agent-sessions/{id}/attachments/read",
            post(permissions::read_attachment),
        )
        .route(
            "/health",
            get(|| async { axum::Json(serde_json::json!({"status":"ok","backend":"rust"})) }),
        )
        .route("/ready", get(ready))
        .nest(
            "/v1",
            protected
                .route("/auth/login", post(auth::login))
                .route("/auth/register", post(auth::register)),
        )
        .route("/internal/agent-sessions/{id}", get(agent_sessions::load))
        .route(
            "/internal/agent-sessions/{id}/actions",
            post(permissions::call),
        )
        .route(
            "/internal/agent-sessions/{id}/deltas",
            post(agent_sessions::save),
        )
        .route(
            "/internal/agent-sessions/{id}/tools",
            post(integrations::call),
        )
        .route(
            "/internal/agent-sessions/{id}/documents/{kind}",
            get(agent_sessions::document_get).put(agent_sessions::document_put),
        )
        .layer(DefaultBodyLimit::max(8 * 1024 * 1024))
        .layer(middleware::map_response(
            |mut response: axum::response::Response| async {
                response
                    .headers_mut()
                    .insert("cache-control", "no-store".parse().unwrap());
                response
            },
        ))
        .with_state(app)
}

pub async fn run() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    dotenvy::from_filename(".runtime/deepseek.env").ok();
    dotenvy::from_filename(".runtime/integrations.env").ok();
    let app = App::connect().await?;
    match std::env::args().nth(1).as_deref() {
        Some("migrate") => {
            platform::migrate(&app).await?;
            return Ok(());
        }
        Some("seed") => {
            auth::seed(&app).await?;
            return Ok(());
        }
        Some("storage-init") => {
            objects::initialize(&app).await?;
            return Ok(());
        }
        Some("worker") => {
            platform::ready(&app).await?;
            anyhow::ensure!(
                app.model_key.is_some(),
                "Standalone agent worker requires DEEPSEEK_API_KEY"
            );
            let concurrency = worker::max_concurrent_runs()?;
            let stop = Arc::new(tokio_util::sync::CancellationToken::new());
            let worker = tokio::spawn(worker::run(app, stop.clone(), concurrency));
            shutdown_signal().await;
            stop.cancel();
            worker.await?;
            return Ok(());
        }
        _ => {}
    }
    platform::ready(&app).await?;
    let routes = router(app.clone());
    let listener = tokio::net::TcpListener::bind(format!("{}:{}", app.host, app.port)).await?;
    let stop = Arc::new(tokio_util::sync::CancellationToken::new());
    let worker = match platform::env("AGENT_WORKER_ENABLED", "1").as_str() {
        "1" => Some(tokio::spawn(worker::run(
            app.clone(),
            stop.clone(),
            worker::max_concurrent_runs()?,
        ))),
        "0" => None,
        _ => anyhow::bail!("AGENT_WORKER_ENABLED must be 0 or 1"),
    };
    let sweeper = tokio::spawn(sandboxes::sweep(app.clone(), stop.clone()));
    axum::serve(listener, routes)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            stop.cancel();
            // Runtime checkpoints use this HTTP server. Drain workers before
            // stopping the listener, otherwise shutdown prevents their saves.
            if let Some(worker) = worker {
                let _ = worker.await;
            }
            let _ = sweeper.await;
        })
        .await?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("SIGTERM handler");
        tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = term.recv() => {} }
    }
    #[cfg(not(unix))]
    tokio::signal::ctrl_c().await.ok();
}
async fn ready(
    axum::extract::State(app): axum::extract::State<App>,
) -> Result<axum::Json<serde_json::Value>> {
    platform::ready(&app).await?;
    Ok(axum::Json(serde_json::json!({"status":"ready"})))
}
