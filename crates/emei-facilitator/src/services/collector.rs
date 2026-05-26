// This module implements the auto-collection background service that periodically
// scans for invoices eligible for collection based on mandate rules and submits
// collect transactions on their behalf.

use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use tokio_util::sync::CancellationToken;

use crate::contracts::invoice::IEMEIInvoice;
use crate::contracts::mandate::IEMEIMandate;
use crate::error::EmeiError;
use crate::state::AppState;

/// Background task that runs a loop to automatically collect eligible invoices based on mandates.
pub async fn auto_collector(state: Arc<AppState>, cancel: CancellationToken) {
    // Stagger startup: wait 10s to avoid RPC rate limits on boot
    tokio::time::sleep(Duration::from_secs(10)).await;

    let interval = Duration::from_secs(state.config.collect_interval);
    let mut ticker = tokio::time::interval(interval);

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!(service = "auto_collector", "shutting down");
                break;
            }
            _ = ticker.tick() => {
                if let Err(e) = collect_cycle(&state).await {
                    tracing::error!(service = "auto_collector", error = %e, "collection cycle failed");
                }
            }
        }
    }
}

async fn collect_cycle(state: &AppState) -> Result<(), EmeiError> {
    // 1. Get total invoice count from chain
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

    tracing::debug!(
        service = "auto_collector",
        total_invoices,
        "scanning for collectible invoices"
    );

    // 2. Scan invoices (from most recent backwards, limited to last 20 for performance)
    let scan_start = total_invoices.saturating_sub(20) + 1;

    for id in (scan_start..=total_invoices).rev() {
        // Rate limit: yield to other tasks and avoid RPC flooding
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        // Fetch invoice
        let invoice = match get_invoice(state, id).await {
            Ok(inv) => inv,
            Err(_) => continue,
        };

        // Only process PRESENTED (status=1) invoices with mandate collection mode (0)
        if invoice.status != 1 || invoice.collectionMode != 0 {
            continue;
        }

        // Skip if invoice is already past due (let the overdue_scanner handle it)
        let presented_at: u64 = invoice.presentedAt.try_into().unwrap_or(0);
        if presented_at > 0 {
            let now_secs = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs();
            let due_at = match invoice.terms.termType {
                0 => presented_at + 300, // due_on_receipt: 5 min grace
                1 => {
                    let net_days: u64 = invoice.terms.netDays.try_into().unwrap_or(1);
                    presented_at + (net_days * 86400)
                }
                _ => presented_at + 300,
            };
            if now_secs > due_at {
                tracing::debug!(
                    service = "auto_collector",
                    invoice_id = id,
                    "skipping overdue invoice"
                );
                continue;
            }
        }

        // Skip if we already submitted a collect tx for this invoice (check DB)
        if state
            .db
            .has_event_for_invoice(id, "InvoicePaid")
            .await
            .unwrap_or(false)
        {
            continue;
        }

        tracing::debug!(
            service = "auto_collector",
            invoice_id = id,
            payer = %invoice.payer,
            issuer = %invoice.issuer,
            "found PRESENTED mandate-mode invoice"
        );

        // 3. Find matching mandate for this payer
        match find_matching_mandate(state, &invoice).await {
            Ok(Some(mandate_id)) => {
                tracing::info!(
                    service = "auto_collector",
                    invoice_id = id,
                    mandate_id,
                    "collecting invoice via mandate"
                );

                // 4. Enqueue collect tx to the tx_queue (confirmed by tx_sender workers)
                let calldata = IEMEIInvoice::collectCall {
                    invoiceId: U256::from(id),
                    mandateId: U256::from(mandate_id),
                }
                .abi_encode();

                match state
                    .enqueue_tx(state.config.invoice_address, calldata, 2, "collector")
                    .await
                {
                    Ok(job_id) => {
                        tracing::info!(
                            service = "auto_collector",
                            invoice_id = id,
                            mandate_id,
                            job_id,
                            "collection enqueued"
                        );

                        // Insert pending event (webhook will confirm it later)
                        let now = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs();
                        let _ = state
                            .db
                            .insert_event(&crate::db::IndexedEvent {
                                event_type: "InvoicePaid".to_string(),
                                block_number: now,
                                tx_hash: format!("pending-collect-job-{}", job_id),
                                log_index: 0,
                                timestamp: now,
                                invoice_id: Some(id),
                                payer: Some(format!("0x{}", hex::encode(invoice.payer))),
                                issuer: Some(format!("0x{}", hex::encode(invoice.issuer))),
                                amount: Some(invoice.amount.to_string()),
                                params: serde_json::json!({"mandate_id": mandate_id, "source": "auto_collector", "job_id": job_id}).to_string(),
                                status: "pending".to_string(),
                            })
                            .await;
                    }
                    Err(e) => {
                        tracing::warn!(
                            service = "auto_collector",
                            invoice_id = id,
                            error = %e,
                            "collection enqueue failed"
                        );
                    }
                }
            }
            Ok(None) => {
                tracing::debug!(
                    service = "auto_collector",
                    invoice_id = id,
                    "no matching mandate found"
                );
            }
            Err(e) => {
                tracing::warn!(
                    service = "auto_collector",
                    invoice_id = id,
                    error = %e,
                    "mandate lookup failed"
                );
            }
        }
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

/// Fetch a mandate from the chain by ID.
async fn find_matching_mandate(
    state: &AppState,
    invoice: &IEMEIInvoice::Invoice,
) -> Result<Option<u64>, EmeiError> {
    // Get all mandate IDs for this payer
    let calldata = IEMEIMandate::getMandatesByPayerCall {
        payer: invoice.payer,
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.mandate_address, calldata.into())
        .await?;

    let mandate_ids = IEMEIMandate::getMandatesByPayerCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("decode getMandatesByPayer: {e}")))?;

    // Check each mandate for a match
    for mandate_id_u256 in mandate_ids.iter() {
        let mandate_id: u64 = (*mandate_id_u256).try_into().unwrap_or(0);
        if mandate_id == 0 {
            continue;
        }

        let mandate = match get_mandate(state, mandate_id).await {
            Ok(m) => m,
            Err(_) => continue,
        };

        // Check mandate is active (status=0 typically means Active)
        if mandate.status != 0 {
            continue;
        }

        // Check time window
        let now = U256::from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        );

        if now < mandate.validFrom || now > mandate.validUntil {
            continue;
        }

        // Check remaining cap
        if mandate.remainingCap < invoice.amount {
            continue;
        }

        // Check counterparty (issuer must be in approved list)
        let issuer_approved = mandate
            .approvedCounterparties
            .iter()
            .any(|cp| *cp == invoice.issuer);

        if !issuer_approved {
            continue;
        }

        // Check category (at least one invoice line item category must be in approved list, if specified)
        let category_approved = if mandate.approvedCategories.is_empty() {
            true // no category restriction
        } else {
            invoice.lineItems.iter().any(|li| {
                mandate
                    .approvedCategories
                    .iter()
                    .any(|cat| *cat == li.category)
            })
        };

        if !category_approved {
            continue;
        }

        // All checks pass — this mandate matches
        return Ok(Some(mandate_id));
    }

    Ok(None)
}

/// Fetch a mandate from the chain by ID.
async fn get_mandate(state: &AppState, id: u64) -> Result<IEMEIMandate::Mandate, EmeiError> {
    let calldata = IEMEIMandate::getMandateCall {
        mandateId: U256::from(id),
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.mandate_address, calldata.into())
        .await?;

    IEMEIMandate::getMandateCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("decode getMandate({id}): {e}")))
}
