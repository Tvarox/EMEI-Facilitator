/// This module implements the overdue invoice scanner service that periodically checks for
/// invoices that have been presented but not paid within their due time, and marks them as
/// overdue on-chain. It also applies reputation penalties to payers for overdue invoices.
use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::bay8004::IBay8004;
use crate::contracts::invoice::IEMEIInvoice;
use crate::error::EmeiError;
use crate::state::AppState;

/// Background task that runs a loop to scan for overdue invoices and mark them on-chain.
pub async fn overdue_scanner(state: Arc<AppState>, cancel: CancellationToken) {
    // Stagger startup: wait 20s
    tokio::time::sleep(Duration::from_secs(20)).await;

    let interval = Duration::from_secs(state.config.overdue_interval);
    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "overdue_scanner", "shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = scan_cycle(&state).await {
                    tracing::error!(service = "overdue_scanner", error = %e, "scan cycle failed");
                }
            }
        }
    }
}

async fn scan_cycle(state: &AppState) -> Result<(), EmeiError> {
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

    // Scan last 30 invoices for PRESENTED ones past due
    let scan_start = total_invoices.saturating_sub(30) + 1;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    for id in scan_start..=total_invoices {
        // Rate limit: yield to other tasks
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(200)).await;

        let invoice = match get_invoice(state, id).await {
            Ok(inv) => inv,
            Err(_) => continue,
        };

        // Only process PRESENTED (status=1) invoices
        if invoice.status != 1 {
            continue;
        }

        // Check if overdue: for due_on_receipt (termType=0), overdue after 5 minutes
        // For net_n_days (termType=1), overdue after presentedAt + netDays * 86400
        let presented_at: u64 = invoice.presentedAt.try_into().unwrap_or(0);
        if presented_at == 0 {
            continue;
        }

        let due_at = match invoice.terms.termType {
            0 => presented_at + 300, // due_on_receipt: 5 min grace period for demo
            1 => {
                let net_days: u64 = invoice.terms.netDays.try_into().unwrap_or(1);
                presented_at + (net_days * 86400)
            }
            _ => presented_at + 300,
        };

        if now <= due_at {
            continue; // Not overdue yet
        }

        tracing::info!(
            service = "overdue_scanner",
            invoice_id = id,
            payer = %invoice.payer,
            "marking invoice as overdue"
        );

        // Mark overdue on-chain via tx_queue
        let calldata = IEMEIInvoice::markOverdueCall {
            invoiceId: U256::from(id),
        }
        .abi_encode();

        match state
            .enqueue_tx(state.config.invoice_address, calldata, 5, "overdue_scanner")
            .await
        {
            Ok(job_id) => {
                tracing::info!(
                    service = "overdue_scanner",
                    invoice_id = id,
                    job_id,
                    "markOverdue enqueued"
                );

                // Insert event (tx_hash will be filled by indexer once confirmed)
                let _ = state
                    .db
                    .insert_event(&crate::db::IndexedEvent {
                        event_type: "InvoiceOverdue".to_string(),
                        block_number: now,
                        tx_hash: format!("pending:job_{}", job_id),
                        log_index: 0,
                        timestamp: now,
                        invoice_id: Some(id),
                        payer: Some(format!("0x{}", hex::encode(invoice.payer))),
                        issuer: Some(format!("0x{}", hex::encode(invoice.issuer))),
                        amount: Some(invoice.amount.to_string()),
                        params: serde_json::json!({"source": "overdue_scanner"}).to_string(),
                    })
                    .await;

                // Penalize payer reputation via giveFeedback(payer, invoiceId, 0)
                let feedback_calldata = IBay8004::giveFeedbackCall {
                    subject: invoice.payer,
                    invoiceId: U256::from(id),
                    amount: U256::ZERO, // zero amount = negative feedback
                }
                .abi_encode();

                match state
                    .enqueue_tx(
                        state.config.bay8004_address,
                        feedback_calldata,
                        3, // lower priority than markOverdue
                        "overdue_scanner:feedback",
                    )
                    .await
                {
                    Ok(fb_job) => {
                        tracing::info!(
                            service = "overdue_scanner",
                            invoice_id = id,
                            payer = %invoice.payer,
                            job_id = fb_job,
                            "reputation penalty enqueued"
                        );
                    }
                    Err(e) => {
                        tracing::warn!(
                            service = "overdue_scanner",
                            invoice_id = id,
                            error = %e,
                            "reputation penalty enqueue failed"
                        );
                    }
                }
            }
            Err(e) => {
                tracing::warn!(
                    service = "overdue_scanner",
                    invoice_id = id,
                    error = %e,
                    "markOverdue enqueue failed"
                );
            }
        }

        // Rate limit: don't spam the RPC
        tokio::time::sleep(Duration::from_millis(1000)).await;
    }

    Ok(())
}

/// Fetch an invoice from the chain by ID.
async fn get_invoice(state: &AppState, id: u64) -> Result<IEMEIInvoice::Invoice, EmeiError> {
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
