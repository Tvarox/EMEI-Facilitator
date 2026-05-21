// EMEI Facilitator Backend
// Library crate providing Axum HTTP routes and background services
// for the EMEI programmable invoicing protocol on Mantle Sepolia.

pub mod chain;
pub mod config;
pub mod contracts;
pub mod db;
pub mod error;
pub mod merkle;
pub mod routes;
pub mod services;
pub mod signing;
pub mod state;
pub mod types;

use std::sync::Arc;

use state::AppState;

/// Returns an Axum router containing all EMEI HTTP routes.
///
/// The router should be merged into the existing facilitator server
/// or run standalone.
pub fn emei_router(state: Arc<AppState>) -> axum::Router {
    routes::emei_routes().with_state(state)
}

/// Spawns all background services (receipt batcher, auto-collector,
/// overdue scanner, event indexer) and returns their join handles.
///
/// Services are cancelled via the `CancellationToken` stored in `AppState`.
pub fn start_services(state: Arc<AppState>) -> Vec<tokio::task::JoinHandle<()>> {
    services::start_services(state)
}
