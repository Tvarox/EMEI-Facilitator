//! TX Reaper service — reclaims stuck jobs that were assigned but never confirmed.

use std::sync::Arc;
use std::time::Duration;

use tokio_util::sync::CancellationToken;

use crate::state::AppState;

/// Background task that periodically reclaims stuck tx_queue jobs.
pub async fn tx_reaper(state: Arc<AppState>, cancel: CancellationToken) {
    // Wait for other services to start
    tokio::time::sleep(Duration::from_secs(30)).await;

    let interval = Duration::from_secs(120); // Check every 2 minutes
    let timeout_secs = 120u64; // Jobs stuck for >2 min get reclaimed
    let mut ticker = tokio::time::interval(interval);

    tracing::info!(
        "tx_reaper: started (timeout={}s, interval={}s)",
        timeout_secs,
        120
    );

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("tx_reaper: shutting down");
                break;
            }
            _ = ticker.tick() => {
                match state.db.reclaim_stuck_jobs(timeout_secs).await {
                    Ok(reclaimed) if reclaimed > 0 => {
                        tracing::warn!(reclaimed, "tx_reaper: reclaimed stuck jobs");
                    }
                    Ok(_) => {} // Nothing stuck
                    Err(e) => {
                        tracing::error!(error = %e, "tx_reaper: reclaim failed");
                    }
                }
            }
        }
    }
}
