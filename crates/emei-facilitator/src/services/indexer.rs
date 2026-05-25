/// This module implements the invoice event indexer service that runs on startup, checks if the database is empty, and if so, performs a one-time backfill of all existing invoices from the blockchain. It then sleeps indefinitely, as ongoing indexing is handled by the webhook worker that listens for Alchemy webhook events about invoice creations, presentations, payments, and overdue status updates.
use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::invoice::IEMEIInvoice;
use crate::db::IndexedEvent;
use crate::error::EmeiError;
use crate::state::AppState;

/// Background task that runs on startup to backfill invoice events if DB is empty, then sleeps indefinitely.
pub async fn event_indexer(state: Arc<AppState>, cancel: CancellationToken) {
    tokio::time::sleep(Duration::from_secs(10)).await;

    let has_data = state.db.latest_block().await.unwrap_or(None).unwrap_or(0) > 0;

    if !has_data {
        tracing::info!("indexer: DB empty, running one-time backfill");
        if let Err(e) = backfill(&state).await {
            tracing::warn!(error = %e, "indexer: backfill failed (webhook will handle going forward)");
        }
    } else {
        tracing::info!("indexer: DB has data, skipping backfill (webhook handles updates)");
    }

    // Sleep forever — webhook handles all ongoing indexing
    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("indexer: shutting down");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(3600)) => {}
        }
    }
}

/// One-time backfill of all existing invoices from the blockchain into the database. Runs on startup if DB is empty.
async fn backfill(state: &AppState) -> Result<(), EmeiError> {
    let count_calldata = IEMEIInvoice::getInvoiceCountCall {}.abi_encode();
    let count_result = state
        .chain
        .call(state.config.invoice_address, count_calldata.into())
        .await?;

    let total: u64 = IEMEIInvoice::getInvoiceCountCall::abi_decode_returns(&count_result)
        .map_err(|e| EmeiError::Internal(format!("decode getInvoiceCount: {e}")))?
        .try_into()
        .unwrap_or(0);

    if total == 0 {
        return Ok(());
    }

    tracing::info!(total, "indexer: backfilling invoices");

    for id in 1..=total {
        let calldata = IEMEIInvoice::getInvoiceCall {
            invoiceId: U256::from(id),
        }
        .abi_encode();
        let result = match state
            .chain
            .call(state.config.invoice_address, calldata.into())
            .await
        {
            Ok(r) => r,
            Err(_) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };

        let invoice = match IEMEIInvoice::getInvoiceCall::abi_decode_returns(&result) {
            Ok(inv) => inv,
            Err(_) => continue,
        };

        let issuer = format!("0x{}", hex::encode(invoice.issuer));
        let payer = format!("0x{}", hex::encode(invoice.payer));
        let amount = invoice.amount.to_string();
        let created_ts: u64 = invoice.createdAt.try_into().unwrap_or(0);

        // Insert Created
        let _ = state
            .db
            .upsert_confirmed_event(&IndexedEvent {
                event_type: "InvoiceCreated".to_string(),
                block_number: id,
                tx_hash: format!("backfill-{}-created", id),
                log_index: 0,
                timestamp: created_ts,
                invoice_id: Some(id),
                payer: Some(payer.clone()),
                issuer: Some(issuer.clone()),
                amount: Some(amount.clone()),
                params: "{}".to_string(),
            status: "pending".to_string(),
            })
            .await;

        // Insert Presented if status >= 1
        if invoice.status >= 1 {
            let pts: u64 = invoice.presentedAt.try_into().unwrap_or(created_ts + 1);
            let _ = state
                .db
                .upsert_confirmed_event(&IndexedEvent {
                    event_type: "InvoicePresented".to_string(),
                    block_number: id,
                    tx_hash: format!("backfill-{}-presented", id),
                    log_index: 1,
                    timestamp: pts,
                    invoice_id: Some(id),
                    payer: Some(payer.clone()),
                    issuer: Some(issuer.clone()),
                    amount: Some(amount.clone()),
                    params: "{}".to_string(),
            status: "pending".to_string(),
                })
                .await;
        }

        // Insert Paid if status == 2
        if invoice.status == 2 {
            let _ = state
                .db
                .upsert_confirmed_event(&IndexedEvent {
                    event_type: "InvoicePaid".to_string(),
                    block_number: id,
                    tx_hash: format!("backfill-{}-paid", id),
                    log_index: 2,
                    timestamp: now_ts(),
                    invoice_id: Some(id),
                    payer: Some(payer.clone()),
                    issuer: Some(issuer.clone()),
                    amount: Some(amount.clone()),
                    params: "{}".to_string(),
            status: "pending".to_string(),
                })
                .await;
        }

        // Insert Overdue if status == 3
        if invoice.status == 3 {
            let _ = state
                .db
                .upsert_confirmed_event(&IndexedEvent {
                    event_type: "InvoiceOverdue".to_string(),
                    block_number: id,
                    tx_hash: format!("backfill-{}-overdue", id),
                    log_index: 3,
                    timestamp: now_ts(),
                    invoice_id: Some(id),
                    payer: Some(payer.clone()),
                    issuer: Some(issuer.clone()),
                    amount: Some(amount.clone()),
                    params: "{}".to_string(),
            status: "pending".to_string(),
                })
                .await;
        }

        // Rate limit
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    tracing::info!(total, "indexer: backfill complete");
    Ok(())
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
