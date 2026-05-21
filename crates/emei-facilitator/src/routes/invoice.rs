//! Invoice route handlers: POST /invoice, /present, /pay, /collect

use std::sync::Arc;

use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use axum::{Json, extract::State, http::StatusCode};

use crate::{
    contracts::invoice::IEMEIInvoice, error::EmeiError, signing::UserSigner, state::AppState,
    types::*,
};

/// POST /emei/invoice — Create a new invoice on-chain.
pub async fn create_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<CreateInvoiceRequest>,
) -> Result<(StatusCode, Json<TxResponse>), EmeiError> {
    body.validate()?;

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

    Ok((
        StatusCode::CREATED,
        Json(TxResponse {
            tx_hash: format!("0x{}", hex::encode(tx_hash)),
        }),
    ))
}

/// POST /emei/present — Present an invoice to the payer.
pub async fn present_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<PresentRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let calldata = IEMEIInvoice::presentCall {
        invoiceId: U256::from(body.invoice_id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.invoice_address, calldata.into())
        .await?;

    Ok(Json(TxResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    }))
}

/// POST /emei/pay — Pay an invoice.
pub async fn pay_invoice(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<PayRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let calldata = IEMEIInvoice::payCall {
        invoiceId: U256::from(body.invoice_id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.invoice_address, calldata.into())
        .await?;

    Ok(Json(TxResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    }))
}

/// POST /emei/collect — Collect an invoice via mandate (uses hot wallet).
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
        .send_hot(state.config.invoice_address, calldata.into())
        .await?;

    Ok(Json(TxResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    }))
}
