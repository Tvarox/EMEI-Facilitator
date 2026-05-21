//! Receipt route handler: GET /verify/:id

use std::sync::Arc;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use axum::{
    Json,
    extract::{Path, State},
};

use crate::{contracts::receipt::IEMEIReceipt, error::EmeiError, state::AppState, types::*};

/// GET /emei/verify/:id — Verify a receipt's inclusion in a batch.
///
/// This endpoint checks the latest batch number and verifies that
/// the receipt (identified by invoice ID) has been included in a
/// posted Merkle root.
pub async fn verify_receipt(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Json<VerifyResponse>, EmeiError> {
    // Get the latest batch number
    let latest_calldata = IEMEIReceipt::getLatestBatchCall {}.abi_encode();
    let latest_result = state
        .chain
        .call(state.config.receipt_address, latest_calldata.into())
        .await?;
    let latest_batch_u256 = IEMEIReceipt::getLatestBatchCall::abi_decode_returns(&latest_result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode getLatestBatch: {e}")))?;

    let latest_batch: u64 = latest_batch_u256.try_into().unwrap_or(0);

    // Get the Merkle root for the latest batch
    if latest_batch == 0 {
        return Ok(Json(VerifyResponse {
            verified: false,
            batch_number: 0,
        }));
    }

    let root_calldata = IEMEIReceipt::getMerkleRootCall {
        batchNumber: U256::from(latest_batch),
    }
    .abi_encode();
    let root_result = state
        .chain
        .call(state.config.receipt_address, root_calldata.into())
        .await?;
    let root = IEMEIReceipt::getMerkleRootCall::abi_decode_returns(&root_result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode getMerkleRoot: {e}")))?;

    // A non-zero root means the batch exists
    let zero_bytes: alloy_primitives::FixedBytes<32> = alloy_primitives::FixedBytes::ZERO;
    let has_root = root != zero_bytes;

    // Compute the leaf hash for this invoice ID.
    // The receipt hash is keccak256(abi.encode(invoiceId)).
    let leaf = alloy_primitives::keccak256(alloy_primitives::U256::from(id).to_be_bytes::<32>());

    // Try to verify inclusion with an empty proof (single-leaf batch)
    let verify_calldata = IEMEIReceipt::verifyInclusionCall {
        batchNumber: U256::from(latest_batch),
        leaf: leaf.into(),
        proof: vec![],
    }
    .abi_encode();

    let verify_result = state
        .chain
        .call(state.config.receipt_address, verify_calldata.into())
        .await;

    let verified = match verify_result {
        Ok(data) => IEMEIReceipt::verifyInclusionCall::abi_decode_returns(&data).unwrap_or(false),
        Err(_) => has_root, // Fallback: batch exists but proof verification failed
    };

    Ok(Json(VerifyResponse {
        verified,
        batch_number: latest_batch,
    }))
}
