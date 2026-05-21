//! Receipt batcher service.
//!
//! Periodically drains the in-memory receipt queue, builds a Merkle tree,
//! and posts the root on-chain via the EMEIReceipt contract.

use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::receipt::IEMEIReceipt;
use crate::error::EmeiError;
use crate::merkle::MerkleTree;
use crate::state::AppState;

/// Background service that batches pending receipt hashes into a Merkle tree
/// and posts the root on-chain at a configurable interval (default 30s).
pub async fn receipt_batcher(state: Arc<AppState>, cancel: CancellationToken) {
    let interval = Duration::from_secs(state.config.batch_interval);
    let mut ticker = tokio::time::interval(interval);
    let mut batch_number: u64 = 1;

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                // Flush remaining receipts before exit
                let _ = flush_batch(&state, &mut batch_number).await;
                tracing::info!(service = "receipt_batcher", "shutting down, flushed final batch");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = flush_batch(&state, &mut batch_number).await {
                    tracing::error!(service = "receipt_batcher", error = %e, "batch cycle failed");
                }
            }
        }
    }
}

async fn flush_batch(state: &AppState, batch_number: &mut u64) -> Result<(), EmeiError> {
    let receipts = state.receipt_queue.drain().await;
    if receipts.is_empty() {
        return Ok(());
    }

    let count = receipts.len();
    let tree = MerkleTree::new(receipts.clone());
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
            // Re-insert receipts on failure so they are retried next cycle
            state.receipt_queue.extend(receipts).await;
            Err(e)
        }
    }
}
