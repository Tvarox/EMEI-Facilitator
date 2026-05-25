//! TX Sender service — wallet pool that claims jobs from tx_queue and confirms them on-chain.

use std::sync::Arc;
use std::time::Duration;

use alloy_network::{EthereumWallet, TransactionBuilder};
use alloy_primitives::{Address, Bytes, B256};
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_signer_local::PrivateKeySigner;
use tokio_util::sync::CancellationToken;

use crate::error::EmeiError;
use crate::state::AppState;

/// Spawn N tx_sender workers, one per wallet key.
pub fn spawn_tx_senders(
    state: Arc<AppState>,
    wallet_keys: Vec<B256>,
    cancel: CancellationToken,
) -> Vec<tokio::task::JoinHandle<()>> {
    wallet_keys
        .into_iter()
        .enumerate()
        .map(|(idx, key)| {
            let state = state.clone();
            let cancel = cancel.clone();
            let wallet_id = format!("wallet_{}", idx);
            tokio::spawn(tx_sender_worker(state, key, wallet_id, cancel))
        })
        .collect()
}

/// Single TX sender worker loop. Owns one wallet, processes jobs sequentially.
async fn tx_sender_worker(
    state: Arc<AppState>,
    wallet_key: B256,
    wallet_id: String,
    cancel: CancellationToken,
) {
    let signer = match PrivateKeySigner::from_bytes(&wallet_key) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(wallet = %wallet_id, error = %e, "tx_sender: invalid wallet key");
            return;
        }
    };

    let wallet_address = signer.address();
    tracing::info!(
        wallet = %wallet_id,
        address = %format!("0x{}", hex::encode(wallet_address)),
        "tx_sender: started"
    );

    loop {
        if cancel.is_cancelled() {
            tracing::info!(wallet = %wallet_id, "tx_sender: shutting down");
            break;
        }

        // Try to claim a job
        match state.db.claim_tx_job(&wallet_id).await {
            Ok(Some(job)) => {
                tracing::info!(
                    wallet = %wallet_id,
                    job_id = job.id,
                    source = %job.source,
                    priority = job.priority,
                    "tx_sender: claimed job"
                );

                // Process the job
                match process_job(&state, &signer, &wallet_id, &job).await {
                    Ok(result) => {
                        tracing::info!(
                            wallet = %wallet_id,
                            job_id = job.id,
                            tx = %result.tx_hash,
                            block = result.block_number,
                            "tx_sender: confirmed"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            wallet = %wallet_id,
                            job_id = job.id,
                            error = %e,
                            "tx_sender: job failed"
                        );
                        let _ = state.db.mark_tx_failed(job.id, &e.to_string()).await;
                    }
                }
            }
            Ok(None) => {
                // No jobs available — sleep before polling again
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
            Err(e) => {
                tracing::error!(wallet = %wallet_id, error = %e, "tx_sender: claim failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

/// Process a single job: sign, send, wait for receipt.
async fn process_job(
    state: &AppState,
    signer: &PrivateKeySigner,
    wallet_id: &str,
    job: &crate::db::tx_queue::TxJob,
) -> Result<crate::db::tx_queue::TxResult, EmeiError> {
    let to: Address = job.to_address.parse().map_err(|_| {
        EmeiError::Internal(format!(
            "invalid to_address in job {}: {}",
            job.id, job.to_address
        ))
    })?;

    let calldata = Bytes::from(job.calldata.clone());
    let wallet = EthereumWallet::from(signer.clone());

    // Build transaction (let provider fill nonce + gas)
    let tx = TransactionRequest::default()
        .with_to(to)
        .with_input(calldata);

    let provider = ProviderBuilder::new()
        .wallet(wallet)
        .connect_provider(&state.rpc_provider);

    // Send transaction
    let pending = tokio::time::timeout(Duration::from_secs(30), provider.send_transaction(tx))
        .await
        .map_err(|_| EmeiError::RpcTimeout)?
        .map_err(|e| EmeiError::RpcError(format!("send_transaction: {e}")))?;

    let tx_hash = format!("0x{}", hex::encode(pending.tx_hash()));

    // Mark as submitted
    let nonce = 0u64; // Provider auto-filled nonce, we don't track it explicitly anymore
    state.db.mark_tx_submitted(job.id, &tx_hash, nonce).await?;

    tracing::debug!(
        wallet = %wallet_id,
        job_id = job.id,
        tx = %tx_hash,
        "tx_sender: submitted, waiting for receipt"
    );

    // Wait for receipt (up to 60s)
    let receipt = tokio::time::timeout(
        Duration::from_secs(60),
        provider.get_transaction_receipt(*pending.tx_hash()),
    )
    .await
    .map_err(|_| EmeiError::Internal("receipt timeout (60s)".into()))?
    .map_err(|e| EmeiError::RpcError(format!("get_receipt: {e}")))?;

    // If receipt is None, poll until we get it
    let receipt = match receipt {
        Some(r) => r,
        None => {
            // Poll for receipt
            let mut attempts = 0;
            loop {
                tokio::time::sleep(Duration::from_secs(2)).await;
                attempts += 1;
                if attempts > 30 {
                    return Err(EmeiError::Internal(
                        "receipt not found after 60s polling".into(),
                    ));
                }
                match provider.get_transaction_receipt(*pending.tx_hash()).await {
                    Ok(Some(r)) => break r,
                    Ok(None) => continue,
                    Err(e) => {
                        if attempts > 10 {
                            return Err(EmeiError::RpcError(format!("get_receipt poll: {e}")));
                        }
                    }
                }
            }
        }
    };

    let block_number = receipt.block_number.unwrap_or(0);

    // Mark as confirmed
    state.db.mark_tx_confirmed(job.id, block_number).await?;

    Ok(crate::db::tx_queue::TxResult {
        job_id: job.id,
        tx_hash,
        block_number,
    })
}
