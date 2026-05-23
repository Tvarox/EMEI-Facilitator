//! Receipt batcher service.
//!
//! Periodically drains pending receipt hashes from the persistent DB queue,
//! builds a Merkle tree, and posts the root on-chain via the EMEIReceipt contract.
//!
//! Receipts are persisted to DB BEFORE being acknowledged, so a crash between
//! collection and batching does not lose receipts.

use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::receipt::IEMEIReceipt;
use crate::error::EmeiError;
use crate::merkle::MerkleTree;
use crate::state::AppState;

/// Maximum receipts per Merkle batch. Keeps gas costs bounded.
const MAX_BATCH_SIZE: usize = 500;

/// Background service that batches pending receipt hashes into a Merkle tree
/// and posts the root on-chain at a configurable interval (default 30s).
///
/// Reads from the persistent `pending_receipts` DB table, not in-memory queue.
pub async fn receipt_batcher(state: Arc<AppState>, cancel: CancellationToken) {
    let interval = Duration::from_secs(state.config.batch_interval);
    let mut ticker = tokio::time::interval(interval);

    // Query the latest batch number from the chain to avoid conflicts
    let mut batch_number: u64 = match get_latest_batch_from_chain(&state).await {
        Ok(n) => {
            tracing::info!(
                service = "receipt_batcher",
                latest_batch = n,
                "resuming from chain state"
            );
            n + 1
        }
        Err(e) => {
            tracing::warn!(
                service = "receipt_batcher",
                error = %e,
                "could not query latest batch, retrying in 30s"
            );
            // Wait and retry once
            tokio::time::sleep(Duration::from_secs(30)).await;
            match get_latest_batch_from_chain(&state).await {
                Ok(n) => n + 1,
                Err(_) => 1,
            }
        }
    };

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "receipt_batcher", "shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = flush_batch(&state, &mut batch_number).await {
                    tracing::error!(service = "receipt_batcher", error = %e, "batch cycle failed");
                    // Exponential backoff on failure: wait extra before next tick
                    tokio::time::sleep(Duration::from_secs(10)).await;
                }
            }
        }
    }
}

/// Query the EMEIReceipt contract for the latest batch number.
async fn get_latest_batch_from_chain(state: &AppState) -> Result<u64, EmeiError> {
    let calldata = IEMEIReceipt::getLatestBatchCall {}.abi_encode();
    let result = state
        .chain
        .call(state.config.receipt_address, calldata.into())
        .await?;
    let batch_u256 = IEMEIReceipt::getLatestBatchCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("decode getLatestBatch: {e}")))?;
    Ok(batch_u256.try_into().unwrap_or(0))
}

async fn flush_batch(state: &AppState, batch_number: &mut u64) -> Result<(), EmeiError> {
    // Drain from persistent DB queue (up to MAX_BATCH_SIZE)
    let receipts = state.db.drain_pending_receipts(MAX_BATCH_SIZE).await?;

    // Also drain any in-memory receipts (legacy path / fast path)
    let mut all_receipts = receipts;
    let mem_receipts = state.receipt_queue.drain().await;
    all_receipts.extend(mem_receipts);

    if all_receipts.is_empty() {
        return Ok(());
    }

    let count = all_receipts.len();
    let tree = MerkleTree::new(all_receipts.clone());
    let root = tree.root();

    let calldata = IEMEIReceipt::postMerkleRootCall {
        batchNumber: U256::from(*batch_number),
        merkleRoot: root.into(),
    }
    .abi_encode();

    match state
        .chain
        .send_hot(state.config.receipt_address, calldata.into())
        .await
    {
        Ok(tx_hash) => {
            tracing::info!(
                service = "receipt_batcher",
                batch = *batch_number,
                receipts = count,
                root = hex::encode(root),
                tx = hex::encode(tx_hash),
                "batch posted"
            );
            *batch_number += 1;
            Ok(())
        }
        Err(e) => {
            // Re-insert receipts to DB on failure so they are retried next cycle
            for receipt in &all_receipts {
                let _ = state.db.insert_pending_receipt(receipt, None).await;
            }
            // If it's a contract revert (batch already exists), re-sync batch number
            if matches!(&e, EmeiError::ContractRevert { .. }) || e.to_string().contains("revert") {
                if let Ok(latest) = get_latest_batch_from_chain(state).await {
                    *batch_number = latest + 1;
                    tracing::info!(
                        service = "receipt_batcher",
                        new_batch_number = *batch_number,
                        "re-synced batch number after revert"
                    );
                }
            }
            Err(e)
        }
    }
}
