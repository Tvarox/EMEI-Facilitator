// Health check route for EMEI Facilitator

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

/// Health check endpoint to verify the status of the EMEI Facilitator server and its dependencies.
pub async fn health_check(State(state): State<Arc<AppState>>) -> Json<HealthResponse> {
    // Check if the Ethereum RPC is reachable by making a simple call to get the invoice count.
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

    // Check if the database is writable by attempting to get the last indexed block number.
    let (db_writable, last_indexed_id) = match state.db.get_last_block().await {
        Ok(Some(block)) => (true, block),
        Ok(None) => (true, 0),
        Err(_) => (false, 0),
    };

    // Get the number of pending receipts in the queue.
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
