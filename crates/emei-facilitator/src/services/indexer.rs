//! Event indexer service.
//!
//! Continuously polls the RPC node for contract events and persists
//! them to the SQLite database for use by the statement endpoint
//! and other background services.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Background service that indexes on-chain events into the local SQLite
/// database. Uses exponential backoff on connection failures (1s → 60s cap).
pub async fn event_indexer(state: Arc<AppState>, cancel: CancellationToken) {
    let mut backoff = Duration::from_secs(1);
    let max_backoff = Duration::from_secs(60);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "event_indexer", "shutting down");
                break;
            }
            _ = run_indexer(&state) => {
                // If run_indexer returns, it means the connection dropped
                tracing::warn!(
                    service = "event_indexer",
                    backoff_secs = backoff.as_secs(),
                    "connection lost, reconnecting"
                );
                tokio::time::sleep(backoff).await;
                backoff = (backoff * 2).min(max_backoff);
            }
        }
    }
}

async fn run_indexer(_state: &AppState) {
    // Placeholder: would subscribe to contract events via polling.
    // For now, just sleep indefinitely (will be woken by cancellation).
    tracing::info!(
        service = "event_indexer",
        "started (polling mode placeholder)"
    );
    loop {
        tokio::time::sleep(Duration::from_secs(3600)).await;
    }
}
