use crate::platform::App;
use axum::{extract::State, response::IntoResponse};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static PROCESS_START: OnceLock<f64> = OnceLock::new();

fn process_start_seconds() -> f64 {
    *PROCESS_START.get_or_init(|| {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock is before Unix epoch")
            .as_secs_f64()
    })
}

fn bool_value(value: bool) -> u8 {
    u8::from(value)
}

pub async fn metrics(State(app): State<App>) -> impl IntoResponse {
    let status = app.db.status();
    let cube_enabled = crate::sandboxes::bridge::enabled();
    let cube_configured = crate::sandboxes::bridge::configured();
    let body = format!(
        "# HELP forge_api_info Build and backend information.\n\
# TYPE forge_api_info gauge\n\
forge_api_info{{backend=\"rust\",version=\"{}\"}} 1\n\
# HELP forge_api_up Whether the Forge API process is serving metrics.\n\
# TYPE forge_api_up gauge\n\
forge_api_up 1\n\
# HELP forge_api_process_start_time_seconds Unix start time of the Forge API process.\n\
# TYPE forge_api_process_start_time_seconds gauge\n\
forge_api_process_start_time_seconds {}\n\
# HELP forge_api_database_pool_size Current PostgreSQL pool size.\n\
# TYPE forge_api_database_pool_size gauge\n\
forge_api_database_pool_size {}\n\
# HELP forge_api_database_pool_max_size Configured PostgreSQL pool maximum.\n\
# TYPE forge_api_database_pool_max_size gauge\n\
forge_api_database_pool_max_size {}\n\
# HELP forge_api_database_pool_available PostgreSQL connections currently available.\n\
# TYPE forge_api_database_pool_available gauge\n\
forge_api_database_pool_available {}\n\
# HELP forge_api_database_pool_waiting Tasks waiting for a PostgreSQL connection.\n\
# TYPE forge_api_database_pool_waiting gauge\n\
forge_api_database_pool_waiting {}\n\
# HELP forge_api_execution_backend Whether the configured Agent execution backend is active.\n\
# TYPE forge_api_execution_backend gauge\n\
forge_api_execution_backend{{backend=\"cube\"}} {}\n\
# HELP forge_api_cube_resident_configured Whether Cube resident configuration is complete.\n\
# TYPE forge_api_cube_resident_configured gauge\n\
forge_api_cube_resident_configured {}\n",
        env!("CARGO_PKG_VERSION"),
        process_start_seconds(),
        status.size,
        status.max_size,
        status.available,
        status.waiting,
        bool_value(cube_enabled),
        bool_value(cube_configured),
    );
    (
        [("content-type", "text/plain; version=0.0.4; charset=utf-8")],
        body,
    )
}
