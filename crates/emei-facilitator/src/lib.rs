pub mod chain;
pub mod config;
pub mod contracts;
pub mod db;
pub mod error;
pub mod merkle;
pub mod redis_client;
pub mod routes;
pub mod services;
pub mod signing;
pub mod state;
pub mod types;

use std::sync::Arc;

use state::AppState;

pub fn emei_router(state: Arc<AppState>) -> axum::Router {
    routes::emei_routes().with_state(state)
}

pub fn start_services(state: Arc<AppState>) -> Vec<tokio::task::JoinHandle<()>> {
    services::start_services(state)
}
