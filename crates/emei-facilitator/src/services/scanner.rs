//! Overdue scanner service.
//!
//! Periodically scans for PRESENTED invoices that have passed their
//! due date and marks them as OVERDUE on-chain.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::error::EmeiError;
use crate::state::AppState;

/// Background service that scans for overdue invoices and marks them
/// on-chain via the hot wallet (default interval: 60s).
pub async fn overdue_scanner(state: Arc<AppState>, cancel: CancellationToken) {
    let interval = Duration::from_secs(state.config.overdue_interval);
    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "overdue_scanner", "shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = scan_cycle(&state).await {
                    tracing::error!(service = "overdue_scanner", error = %e, "scan cycle failed");
                }
            }
        }
    }
}

async fn scan_cycle(_state: &AppState) -> Result<(), EmeiError> {
    // Placeholder: would query DB for PRESENTED invoices past due date
    // and call markOverdue() via hot wallet.
    tracing::debug!(
        service = "overdue_scanner",
        "cycle complete (no-op until indexer populates DB)"
    );
    Ok(())
}
