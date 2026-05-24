//! Event indexer service.
//!
//! Continuously polls the chain for invoice state and persists events
//! to the SQLite database. Re-scans recent invoices to catch state
//! transitions (ISSUED → PRESENTED → PAID).

use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::invoice::IEMEIInvoice;
use crate::db::IndexedEvent;
use crate::error::EmeiError;
use crate::state::AppState;

/// Background service that indexes on-chain events into the local SQLite
/// database. Polls every 15 seconds. Re-scans recent invoices to catch
/// state transitions.
pub async fn event_indexer(state: Arc<AppState>, cancel: CancellationToken) {
    // Stagger startup
    tokio::time::sleep(Duration::from_secs(5)).await;

    let poll_interval = Duration::from_secs(15);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "event_indexer", "shutting down");
                break;
            }
            _ = tokio::time::sleep(poll_interval) => {
                if let Err(e) = index_cycle(&state).await {
                    tracing::warn!(
                        service = "event_indexer",
                        error = %e,
                        "indexing cycle failed"
                    );
                }
            }
        }
    }
}

async fn index_cycle(state: &AppState) -> Result<(), EmeiError> {
    // Get total invoice count
    let count_calldata = IEMEIInvoice::getInvoiceCountCall {}.abi_encode();
    let count_result = state
        .chain
        .call(state.config.invoice_address, count_calldata.into())
        .await?;

    let total_invoices: u64 = IEMEIInvoice::getInvoiceCountCall::abi_decode_returns(&count_result)
        .map_err(|e| EmeiError::Internal(format!("decode getInvoiceCount: {e}")))?
        .try_into()
        .unwrap_or(0);

    if total_invoices == 0 {
        return Ok(());
    }

    // Get the last fully indexed invoice ID
    let last_indexed = state.db.get_last_block().await?.unwrap_or(0);

    // Strategy:
    // 1. Index any NEW invoices (last_indexed+1 to total)
    // 2. Re-scan the last 20 invoices to catch state transitions (PRESENTED → PAID)

    // Part 1: Index new invoices
    if last_indexed < total_invoices {
        let start = last_indexed + 1;
        tracing::info!(
            service = "event_indexer",
            from = start,
            to = total_invoices,
            "indexing new invoices"
        );

        for id in start..=total_invoices {
            if let Err(e) = index_invoice(state, id).await {
                tracing::warn!(
                    service = "event_indexer",
                    invoice_id = id,
                    error = %e,
                    "failed to index invoice"
                );
                break;
            }
            state.db.set_last_block(id).await?;
            tokio::time::sleep(Duration::from_millis(800)).await;
        }
    }

    // Part 2: Re-scan last 20 invoices to catch state changes
    let rescan_start = total_invoices.saturating_sub(20) + 1;
    for id in rescan_start..=total_invoices {
        if let Err(_) = rescan_invoice(state, id).await {
            // Non-critical: just skip
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(())
}

/// Index a single invoice — inserts events for all observed states.
async fn index_invoice(state: &AppState, id: u64) -> Result<(), EmeiError> {
    let invoice = fetch_invoice(state, id).await?;
    insert_events_for_invoice(state, id, &invoice).await
}

/// Re-scan an already-indexed invoice to catch state transitions.
/// Only inserts events that don't already exist (INSERT OR IGNORE).
async fn rescan_invoice(state: &AppState, id: u64) -> Result<(), EmeiError> {
    let invoice = fetch_invoice(state, id).await?;
    insert_events_for_invoice(state, id, &invoice).await
}

/// Fetch invoice from chain.
async fn fetch_invoice(state: &AppState, id: u64) -> Result<IEMEIInvoice::Invoice, EmeiError> {
    let calldata = IEMEIInvoice::getInvoiceCall {
        invoiceId: U256::from(id),
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.invoice_address, calldata.into())
        .await?;

    IEMEIInvoice::getInvoiceCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("decode getInvoice({id}): {e}")))
}

/// Insert events into DB based on invoice state. Uses INSERT OR IGNORE
/// so duplicate events are harmless.
async fn insert_events_for_invoice(
    state: &AppState,
    id: u64,
    invoice: &IEMEIInvoice::Invoice,
) -> Result<(), EmeiError> {
    let issuer = format!("0x{}", hex::encode(invoice.issuer));
    let payer = format!("0x{}", hex::encode(invoice.payer));
    let amount = invoice.amount.to_string();
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let created_ts: u64 = if invoice.createdAt > U256::ZERO {
        invoice.createdAt.try_into().unwrap_or(now_ts)
    } else {
        now_ts
    };

    let category = invoice
        .lineItems
        .first()
        .map(|li| li.category.clone())
        .unwrap_or_default();

    let collection_mode = match invoice.collectionMode {
        0 => "mandate",
        1 => "pay_link",
        _ => "unknown",
    };

    let params = serde_json::json!({
        "category": category,
        "collection_mode": collection_mode,
        "asset": format!("0x{}", hex::encode(invoice.asset)),
    })
    .to_string();

    // Always insert InvoiceCreated
    state
        .db
        .insert_event(&IndexedEvent {
            event_type: "InvoiceCreated".to_string(),
            block_number: id,
            tx_hash: format!("invoice-{}-created", id),
            log_index: 0,
            timestamp: created_ts,
            invoice_id: Some(id),
            payer: Some(payer.clone()),
            issuer: Some(issuer.clone()),
            amount: Some(amount.clone()),
            params: params.clone(),
        })
        .await?;

    // If presented (status >= 1)
    if invoice.status >= 1 {
        let presented_ts: u64 = if invoice.presentedAt > U256::ZERO {
            invoice.presentedAt.try_into().unwrap_or(created_ts + 1)
        } else {
            created_ts + 1
        };

        state
            .db
            .insert_event(&IndexedEvent {
                event_type: "InvoicePresented".to_string(),
                block_number: id,
                tx_hash: format!("invoice-{}-presented", id),
                log_index: 1,
                timestamp: presented_ts,
                invoice_id: Some(id),
                payer: Some(payer.clone()),
                issuer: Some(issuer.clone()),
                amount: Some(amount.clone()),
                params: params.clone(),
            })
            .await?;
    }

    // If paid (status == 2)
    if invoice.status == 2 {
        state
            .db
            .insert_event(&IndexedEvent {
                event_type: "InvoicePaid".to_string(),
                block_number: id,
                tx_hash: format!("invoice-{}-paid", id),
                log_index: 2,
                timestamp: now_ts,
                invoice_id: Some(id),
                payer: Some(payer.clone()),
                issuer: Some(issuer.clone()),
                amount: Some(amount.clone()),
                params: params.clone(),
            })
            .await?;
    }

    // If overdue (status == 3)
    if invoice.status == 3 {
        state
            .db
            .insert_event(&IndexedEvent {
                event_type: "InvoiceOverdue".to_string(),
                block_number: id,
                tx_hash: format!("invoice-{}-overdue", id),
                log_index: 3,
                timestamp: now_ts,
                invoice_id: Some(id),
                payer: Some(payer.clone()),
                issuer: Some(issuer.clone()),
                amount: Some(amount.clone()),
                params: params.clone(),
            })
            .await?;
    }

    Ok(())
}
