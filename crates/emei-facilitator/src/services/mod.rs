//! Background services for the EMEI facilitator.
//!
//! Includes receipt batching, auto-collection, overdue scanning,
//! and event indexing. All services share a `CancellationToken` for
//! graceful shutdown.

pub mod batcher;
pub mod collector;
pub mod indexer;
pub mod scanner;

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::state::AppState;

/// Spawn all background services and return their join handles.
///
/// Services are cancelled via the `CancellationToken` stored in `AppState`.
pub fn start_services(state: Arc<AppState>) -> Vec<JoinHandle<()>> {
    let cancel = state.cancel.clone();
    vec![
        tokio::spawn(batcher::receipt_batcher(state.clone(), cancel.clone())),
        tokio::spawn(collector::auto_collector(state.clone(), cancel.clone())),
        tokio::spawn(scanner::overdue_scanner(state.clone(), cancel.clone())),
        tokio::spawn(indexer::event_indexer(state.clone(), cancel)),
    ]
}
