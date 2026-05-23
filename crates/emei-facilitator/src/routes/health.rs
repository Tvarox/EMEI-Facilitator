//! Health check endpoint: GET /health
//!
//! Reports system status for load balancers and monitoring.

use std::sync::Arc;

use axum::{extract::State, Json};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub rpc_reachable: bool,
    pub db_writable: bool,
    pub last_indexed_id: u64,
    pub pending_receipts: usize,
    pub version: String,
    pub chain_id: u64,
}

/// GET /health — System health check.
///
/// Returns 200 with status "healthy" if RPC and DB are reachable.
/// Returns 200 with status "degraded" if either is down (still serves reads).
pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    // Check RPC: try a simple call (getInvoiceCount is cheap)
    let rpc_reachable = state
        .chain
        .call(
            state.config.invoice_address,
            alloy_sol_types::SolCall::abi_encode(
                &crate::contracts::invoice::IEMEIInvoice::getInvoiceCountCall {},
            )
            .into(),
        )
        .await
        .is_ok();

    // Check DB: try reading last block
    let (db_writable, last_indexed_id) = match state.db.get_last_block().await {
        Ok(Some(block)) => (true, block),
        Ok(None) => (true, 0),
        Err(_) => (false, 0),
    };

    // Check receipt queue
    let pending_receipts = state.receipt_queue.len().await;

    let status = if rpc_reachable && db_writable {
        "healthy"
    } else {
        "degraded"
    };

    Json(HealthResponse {
        status: status.to_string(),
        rpc_reachable,
        db_writable,
        last_indexed_id,
        pending_receipts,
        version: env!("CARGO_PKG_VERSION").to_string(),
        chain_id: 5003,
    })
}
