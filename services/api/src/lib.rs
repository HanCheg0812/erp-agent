mod agent_sessions;
mod auth;
mod cases;
mod conversation;
mod integrations;
mod mcp_sessions;
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

fn internal_router(app: App) -> Router {
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
            "/internal/agent-sessions/{id}",
            get(agent_sessions::load),
        )
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
            "/internal/mcp-sessions/{id}/tools",
            post(integrations::call),
        )
        .route(
            "/internal/agent-sessions/{id}/documents/{kind}",
            get(agent_sessions::document_get).put(agent_sessions::document_put),
        )
        .route(
            "/health",
            get(|| async {
                axum::Json(serde_json::json!({
                    "status":"ok",
                    "backend":"rust",
                    "agent_execution_backend": if sandboxes::bridge::enabled() { "cube" } else { "local" },
                    "cube_resident_configured": sandboxes::bridge::configured()
                }))
            }),
        )
        .route("/ready", get(ready))
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
        .route("/mcp-sessions", post(mcp_sessions::create_route))
        .route(
            "/mcp-sessions/{id}",
            axum::routing::delete(mcp_sessions::revoke_route),
        )
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
            get(|| async {
                axum::Json(serde_json::json!({
                    "status":"ok",
                    "backend":"rust",
                    "agent_execution_backend": if sandboxes::bridge::enabled() { "cube" } else { "local" },
                    "cube_resident_configured": sandboxes::bridge::configured()
                }))
            }),
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
            "/internal/mcp-sessions/{id}/tools",
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
    // Cube runtime settings are an explicit deployment overlay. They must
    // override the checked-in local defaults, especially the template ID.
    if std::env::var("ERP_AGENT_LOAD_CUBE_ENV").as_deref() != Ok("0") {
        dotenvy::from_filename_override(".runtime/cube.env").ok();
    }
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
    let internal_listener = if let Ok(host) = std::env::var("API_INTERNAL_HOST") {
        if host.trim().is_empty() {
            None
        } else {
            let port = platform::env("API_INTERNAL_PORT", "3101");
            Some(tokio::net::TcpListener::bind(format!("{}:{}", host, port)).await?)
        }
    } else {
        None
    };
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
    let internal_server = internal_listener.map(|listener| {
        let internal_stop = stop.clone();
        tokio::spawn(async move {
            axum::serve(listener, internal_router(app))
                .with_graceful_shutdown(internal_stop.as_ref().clone().cancelled_owned())
                .await
        })
    });
    let result = axum::serve(listener, routes)
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
        .await;
    if let Some(server) = internal_server {
        let _ = server.await;
    }
    result?;
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
