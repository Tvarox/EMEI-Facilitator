//! Auto-collector service.
//!
//! Periodically scans for PRESENTED invoices that match active mandates
//! and triggers automatic collection on behalf of the payer.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::EmeiError;
use crate::state::AppState;

/// Background service that checks for invoices eligible for automatic
/// collection based on mandate rules (default interval: 10s).
pub async fn auto_collector(state: Arc<AppState>, cancel: CancellationToken) {
    let interval = Duration::from_secs(state.config.collect_interval);
    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "auto_collector", "shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = collect_cycle(&state).await {
                    tracing::error!(service = "auto_collector", error = %e, "collection cycle failed");
                }
            }
        }
    }
}

async fn collect_cycle(_state: &AppState) -> Result<(), EmeiError> {
    // Placeholder: full implementation would query the DB for PRESENTED invoices
    // past due date, match against mandates, and call collect().
    // This requires the event indexer to be running and populating the DB.
    tracing::debug!(
        service = "auto_collector",
        "cycle complete (no-op until indexer populates DB)"
    );
    Ok(())
}
