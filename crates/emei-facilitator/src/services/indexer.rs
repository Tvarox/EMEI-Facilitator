//! Event indexer service.
//!
//! Continuously polls the RPC node for contract events and persists
//! them to the SQLite database for use by the statement and public endpoints.

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
/// database. Polls every 5 seconds for new blocks.
pub async fn event_indexer(state: Arc<AppState>, cancel: CancellationToken) {
    // Stagger startup: wait 5s to avoid RPC rate limits on boot
    tokio::time::sleep(Duration::from_secs(5)).await;

    let poll_interval = Duration::from_secs(15); // Poll every 15s to be gentle on RPC

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

/// One indexing cycle: get the current invoice count, scan for invoices
/// we haven't indexed yet, and insert events for each state we observe.
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

    // Get the last indexed invoice ID from indexer_state
    let last_indexed = state.db.get_last_block().await?.unwrap_or(0);

    // If we've already indexed everything, nothing to do
    if last_indexed >= total_invoices {
        return Ok(());
    }

    let start = last_indexed + 1;
    let end = total_invoices;

    tracing::info!(
        service = "event_indexer",
        from = start,
        to = end,
        "indexing new invoices"
    );

    for id in start..=end {
        if let Err(e) = index_invoice(state, id).await {
            tracing::warn!(
                service = "event_indexer",
                invoice_id = id,
                error = %e,
                "failed to index invoice"
            );
            // Don't advance past this ID — retry next cycle
            break;
        }

        // Update the last indexed ID after each successful invoice
        state.db.set_last_block(id).await?;

        // Rate limit: sleep 1s between RPC calls to avoid 429s
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    Ok(())
}

/// Index a single invoice by reading its on-chain state and inserting
/// appropriate events into the database.
async fn index_invoice(state: &AppState, id: u64) -> Result<(), EmeiError> {
    let calldata = IEMEIInvoice::getInvoiceCall {
        invoiceId: U256::from(id),
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.invoice_address, calldata.into())
        .await?;

    let invoice = IEMEIInvoice::getInvoiceCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("decode getInvoice({id}): {e}")))?;

    let issuer = format!("0x{}", hex::encode(invoice.issuer));
    let payer = format!("0x{}", hex::encode(invoice.payer));
    let amount = invoice.amount.to_string();
    let now_ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    // Use createdAt as the timestamp for the created event
    let created_ts = if invoice.createdAt > U256::ZERO {
        let ts: u64 = invoice.createdAt.try_into().unwrap_or(now_ts);
        ts
    } else {
        now_ts
    };

    // Get category from first line item
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
    let created_event = IndexedEvent {
        event_type: "InvoiceCreated".to_string(),
        block_number: id, // Use invoice ID as a pseudo-block for ordering
        tx_hash: format!("invoice-{}-created", id),
        log_index: 0,
        timestamp: created_ts,
        invoice_id: Some(id),
        payer: Some(payer.clone()),
        issuer: Some(issuer.clone()),
        amount: Some(amount.clone()),
        params: params.clone(),
    };
    state.db.insert_event(&created_event).await?;

    // If presented (status >= 1)
    if invoice.status >= 1 {
        let presented_ts = if invoice.presentedAt > U256::ZERO {
            let ts: u64 = invoice.presentedAt.try_into().unwrap_or(now_ts);
            ts
        } else {
            created_ts + 1
        };

        let presented_event = IndexedEvent {
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
        };
        state.db.insert_event(&presented_event).await?;
    }

    // If paid (status == 2)
    if invoice.status == 2 {
        let paid_event = IndexedEvent {
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
        };
        state.db.insert_event(&paid_event).await?;
    }

    // If overdue (status == 3)
    if invoice.status == 3 {
        let overdue_event = IndexedEvent {
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
        };
        state.db.insert_event(&overdue_event).await?;
    }

    tracing::debug!(
        service = "event_indexer",
        invoice_id = id,
        status = invoice.status,
        "indexed invoice"
    );

    Ok(())
}
