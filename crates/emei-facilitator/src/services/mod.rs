/// This module contains all the background services that run in the facilitator, such as the invoice event indexer, the overdue invoice scanner, and the webhook worker that processes Alchemy webhook payload
/// and updates the database accordingly. Each service runs in its own async task and they are all spawned from the `start_services` function.
pub mod batcher;
pub mod collector;
pub mod indexer;
pub mod scanner;
pub mod webhook_worker;

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::state::AppState;

/// Spawn all background services.
pub fn start_services(state: Arc<AppState>) -> Vec<JoinHandle<()>> {
    let cancel = state.cancel.clone();
    vec![
        tokio::spawn(batcher::receipt_batcher(state.clone(), cancel.clone())),
        tokio::spawn(collector::auto_collector(state.clone(), cancel.clone())),
        tokio::spawn(scanner::overdue_scanner(state.clone(), cancel.clone())),
        tokio::spawn(indexer::event_indexer(state.clone(), cancel.clone())),
        tokio::spawn(webhook_worker::webhook_worker(state.clone(), cancel)),
    ]
}
