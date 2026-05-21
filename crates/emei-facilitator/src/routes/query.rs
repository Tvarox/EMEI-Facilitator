//! Query route handlers: GET /invoice/:id, /statement, /reputation/:address, /balance/:address

use std::sync::Arc;

use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use axum::{
    Json,
    extract::{Path, Query, State},
};

use crate::{
    contracts::{bay8004::IBay8004, invoice::IEMEIInvoice, settlement::IEMEISettlement},
    db::StatementQuery,
    error::EmeiError,
    state::AppState,
    types::*,
};

/// GET /emei/invoice/:id — Fetch invoice details from the chain.
pub async fn get_invoice(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Json<InvoiceResponse>, EmeiError> {
    let calldata = IEMEIInvoice::getInvoiceCall {
        invoiceId: U256::from(id),
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.invoice_address, calldata.into())
        .await?;

    let invoice = IEMEIInvoice::getInvoiceCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode getInvoice result: {e}")))?;

    let status_str = match invoice.status {
        0 => "ISSUED",
        1 => "PRESENTED",
        2 => "PAID",
        3 => "OVERDUE",
        4 => "REJECTED",
        _ => "UNKNOWN",
    };

    let collection_mode_str = match invoice.collectionMode {
        0 => "mandate",
        1 => "pay_link",
        _ => "unknown",
    };

    Ok(Json(InvoiceResponse {
        invoice_id: invoice.invoiceId.to_string(),
        issuer: format!("0x{}", hex::encode(invoice.issuer)),
        payer: format!("0x{}", hex::encode(invoice.payer)),
        amount: invoice.amount.to_string(),
        asset: format!("0x{}", hex::encode(invoice.asset)),
        status: status_str.to_string(),
        collection_mode: collection_mode_str.to_string(),
        settlement_proof: format!("0x{}", hex::encode(invoice.settlementProof)),
        presented_at: invoice.presentedAt.to_string(),
        created_at: invoice.createdAt.to_string(),
    }))
}

/// GET /emei/statement — Query indexed events from the database.
pub async fn get_statement(
    State(state): State<Arc<AppState>>,
    Query(params): Query<StatementQueryParams>,
) -> Result<Json<Vec<crate::db::IndexedEvent>>, EmeiError> {
    let query = StatementQuery {
        payer: params.payer,
        status: params.status,
        from: params.from,
        to: params.to,
        offset: params.offset.unwrap_or(0),
        limit: params.limit.unwrap_or(100),
    };

    let events = state.db.query_statement(&query).await?;
    Ok(Json(events))
}

/// GET /emei/reputation/:address — Get reputation score from Bay8004.
pub async fn get_reputation(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Result<Json<ReputationResponse>, EmeiError> {
    let addr: Address = address.parse().map_err(|_| EmeiError::Validation {
        field: "address".into(),
        reason: "invalid Ethereum address".into(),
    })?;

    let calldata = IBay8004::scoreOfCall { account: addr }.abi_encode();

    let result = state
        .chain
        .call(state.config.bay8004_address, calldata.into())
        .await?;

    let score = IBay8004::scoreOfCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode scoreOf result: {e}")))?;

    Ok(Json(ReputationResponse {
        address: format!("0x{}", hex::encode(addr)),
        score: score.try_into().unwrap_or(0),
    }))
}

/// GET /emei/balance/:address — Get vault balance and accrued yield.
pub async fn get_balance(
    State(state): State<Arc<AppState>>,
    Path(address): Path<String>,
) -> Result<Json<BalanceResponse>, EmeiError> {
    let addr: Address = address.parse().map_err(|_| EmeiError::Validation {
        field: "address".into(),
        reason: "invalid Ethereum address".into(),
    })?;

    // Get vault balance
    let balance_calldata = IEMEISettlement::getVaultBalanceCall { payee: addr }.abi_encode();
    let balance_result = state
        .chain
        .call(state.config.settlement_address, balance_calldata.into())
        .await?;
    let balance = IEMEISettlement::getVaultBalanceCall::abi_decode_returns(&balance_result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode getVaultBalance: {e}")))?;

    // Get accrued yield
    let yield_calldata = IEMEISettlement::getAccruedYieldCall { payee: addr }.abi_encode();
    let yield_result = state
        .chain
        .call(state.config.settlement_address, yield_calldata.into())
        .await?;
    let accrued_yield = IEMEISettlement::getAccruedYieldCall::abi_decode_returns(&yield_result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode getAccruedYield: {e}")))?;

    Ok(Json(BalanceResponse {
        balance: balance.to_string(),
        accrued_yield: accrued_yield.to_string(),
    }))
}
