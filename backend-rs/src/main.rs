use codoxear_backend_rs::app_state::build_state;
use codoxear_backend_rs::routes::router;
use codoxear_backend_rs::runtime::{
    rust_harness_sweep_enabled, rust_queue_sweep_enabled, spawn_harness_sweep_worker,
    spawn_queue_sweep_worker,
};
use std::net::SocketAddr;
use std::{env, net::IpAddr};
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let state = build_state();
    if rust_harness_sweep_enabled() {
        spawn_harness_sweep_worker(state.config.clone());
    }
    if rust_queue_sweep_enabled() {
        spawn_queue_sweep_worker(state.config.clone());
    }
    let app = router(state)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any));

    let host = env::var("CODOXEAR_BIND_HOST")
        .ok()
        .and_then(|value| value.parse::<IpAddr>().ok())
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));
    let port = env::var("CODOXEAR_BIND_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(8787);
    let address = SocketAddr::new(host, port);
    let listener = tokio::net::TcpListener::bind(address).await.unwrap();
    tracing::info!("codoxear-backend-rs listening on http://{address}");
    axum::serve(listener, app).await.unwrap();
}
