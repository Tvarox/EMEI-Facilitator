// Route handlers for invoice-related endpoints in the EMEI Facilitator API.
use std::sync::Arc;

use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use axum::{extract::State, http::StatusCode, Json};

use crate::{
    contracts::invoice::IEMEIInvoice, db::IndexedEvent, error::EmeiError, signing::UserSigner,
    state::AppState, types::*,
};

/// POST /emei/create — Create a new invoice on the blockchain.
pub async fn create_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<CreateInvoiceRequest>,
) -> Result<(StatusCode, Json<TxResponse>), EmeiError> {
    body.validate()?;

    // Get issuer address from signer
    let issuer_address = signer.0.address();
    // Parse and validate input fields
    let payer: Address = body.payer.parse().map_err(|_| EmeiError::Validation {
        field: "payer".into(),
        reason: "invalid address".into(),
    })?;
    let asset: Address = body.asset.parse().map_err(|_| EmeiError::Validation {
        field: "asset".into(),
        reason: "invalid address".into(),
    })?;
    let amount = body
        .amount
        .parse::<U256>()
        .map_err(|_| EmeiError::Validation {
            field: "amount".into(),
            reason: "invalid U256".into(),
        })?;

    let category = body
        .line_items
        .first()
        .map(|li| li.category.clone())
        .unwrap_or_default();

    // Convert line items
    let line_items: Vec<IEMEIInvoice::LineItem> = body
        .line_items
        .iter()
        .map(|li| {
            let li_amount = li.amount.parse::<U256>().unwrap_or(U256::ZERO);
            IEMEIInvoice::LineItem {
                description: li.description.clone(),
                amount: li_amount,
                category: li.category.clone(),
            }
        })
        .collect();

    // Convert terms
    let term_type: u8 = match body.terms.term_type.as_str() {
        "due_on_receipt" => 0,
        "net_n_days" => 1,
        "milestones" => 2,
        _ => 0,
    };
    let net_days = U256::from(body.terms.net_days.unwrap_or(0));
    let milestones: Vec<IEMEIInvoice::Milestone> = body
        .terms
        .milestones
        .as_ref()
        .map(|ms| {
            ms.iter()
                .map(|m| IEMEIInvoice::Milestone {
                    amount: m.amount.parse::<U256>().unwrap_or(U256::ZERO),
                    dueDate: U256::from(m.due_date),
                    description: m.description.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    let collection_mode: u8 = match body.collection_mode.as_str() {
        "mandate" => 0,
        "pay_link" => 1,
        _ => 1,
    };

    let params = IEMEIInvoice::CreateInvoiceParams {
        payer,
        amount,
        asset,
        lineItems: line_items,
        terms: IEMEIInvoice::Terms {
            termType: term_type,
            netDays: net_days,
            milestones,
        },
        collectionMode: collection_mode,
    };

    let calldata = IEMEIInvoice::createInvoiceCall { params }.abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.invoice_address, calldata.into())
        .await?;

    let tx_hash_str = format!("0x{}", hex::encode(tx_hash));

    // Insert real-time event with full context
    let _ = state
        .db
        .insert_event(&IndexedEvent {
            event_type: "InvoiceCreated".to_string(),
            block_number: now_ts(),
            tx_hash: tx_hash_str.clone(),
            log_index: 0,
            timestamp: now_ts(),
            invoice_id: None,
            payer: Some(format!("0x{}", hex::encode(payer))),
            issuer: Some(format!("0x{}", hex::encode(issuer_address))),
            amount: Some(body.amount.clone()),
            params: serde_json::json!({
                "category": category,
                "collection_mode": body.collection_mode,
                "asset": format!("0x{}", hex::encode(asset)),
            })
            .to_string(),
        })
        .await;

    Ok((
        StatusCode::CREATED,
        Json(TxResponse {
            tx_hash: tx_hash_str,
        }),
    ))
}

/// POST /emei/present — Present an invoice to trigger state changes and events.
pub async fn present_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<PresentRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let issuer_address = signer.0.address();

    let calldata = IEMEIInvoice::presentCall {
        invoiceId: U256::from(body.invoice_id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.invoice_address, calldata.into())
        .await?;

    let tx_hash_str = format!("0x{}", hex::encode(tx_hash));

    // Insert real-time event
    let _ = state
        .db
        .insert_event(&IndexedEvent {
            event_type: "InvoicePresented".to_string(),
            block_number: now_ts(),
            tx_hash: tx_hash_str.clone(),
            log_index: 0,
            timestamp: now_ts(),
            invoice_id: Some(body.invoice_id),
            payer: None, // Will be enriched by indexer
            issuer: Some(format!("0x{}", hex::encode(issuer_address))),
            amount: None,
            params: "{}".to_string(),
        })
        .await;

    Ok(Json(TxResponse {
        tx_hash: tx_hash_str,
    }))
}

/// POST /emei/pay — Pay an invoice directly (payer-initiated).
pub async fn pay_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<PayRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let payer_address = signer.0.address();

    let calldata = IEMEIInvoice::payCall {
        invoiceId: U256::from(body.invoice_id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.invoice_address, calldata.into())
        .await?;

    let tx_hash_str = format!("0x{}", hex::encode(tx_hash));

    // Insert real-time event
    let _ = state
        .db
        .insert_event(&IndexedEvent {
            event_type: "InvoicePaid".to_string(),
            block_number: now_ts(),
            tx_hash: tx_hash_str.clone(),
            log_index: 0,
            timestamp: now_ts(),
            invoice_id: Some(body.invoice_id),
            payer: Some(format!("0x{}", hex::encode(payer_address))),
            issuer: None,
            amount: None,
            params: "{}".to_string(),
        })
        .await;

    Ok(Json(TxResponse {
        tx_hash: tx_hash_str,
    }))
}

/// POST /emei/collect — Collect payment for an invoice (issuer-initiated).
pub async fn collect_invoice(
    State(state): State<Arc<AppState>>,
    Json(body): Json<CollectRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let calldata = IEMEIInvoice::collectCall {
        invoiceId: U256::from(body.invoice_id),
        mandateId: U256::from(body.mandate_id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_hot(state.config.invoice_address, calldata.into(), &state.redis)
        .await?;

    let tx_hash_str = format!("0x{}", hex::encode(tx_hash));

    // Insert real-time event
    let _ = state
        .db
        .insert_event(&IndexedEvent {
            event_type: "InvoicePaid".to_string(),
            block_number: now_ts(),
            tx_hash: tx_hash_str.clone(),
            log_index: 0,
            timestamp: now_ts(),
            invoice_id: Some(body.invoice_id),
            payer: None,
            issuer: None,
            amount: None,
            params: serde_json::json!({"mandate_id": body.mandate_id, "source": "collect"})
                .to_string(),
        })
        .await;

    Ok(Json(TxResponse {
        tx_hash: tx_hash_str,
    }))
}

// Helper function to get current timestamp as u64
fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
