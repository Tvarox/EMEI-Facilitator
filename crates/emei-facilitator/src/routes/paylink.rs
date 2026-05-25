/// Route handler for GET /emei/paylink/:id — fetching invoice details and pre-encoded transaction data for the pay-link page.
use std::sync::Arc;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use axum::{
    extract::{Path, State},
    Json,
};
use serde::Serialize;

use crate::{contracts::invoice::IEMEIInvoice, error::EmeiError, state::AppState};

#[derive(Serialize)]
pub struct PayLinkInfo {
    pub invoice_id: u64,
    pub issuer: String,
    pub payer: String,
    pub amount: String,
    pub asset: String,
    pub status: String,
    pub settlement_contract: String,
    pub invoice_contract: String,
    /// Pre-encoded calldata for ERC-20 approve (payer approves Settlement to spend)
    pub approve_calldata: String,
    /// Pre-encoded calldata for EMEIInvoice.pay(invoiceId)
    pub pay_calldata: String,
    /// The address the approve should target (the asset token contract)
    pub approve_to: String,
    /// The address the pay tx should target (EMEIInvoice contract)
    pub pay_to: String,
}

/// GET /emei/paylink/:id — Fetch invoice details and pre-encoded transaction data for the pay-link page.
/// This endpoint is used by the pay-link frontend to retrieve all necessary information to display the invoice and facilitate payment, including pre-encoded calldata for both the ERC-20 approve and the invoice pay transactions.
/// The endpoint checks that the invoice is in a payable state (PRESENTED or OVERDUE) before returning the data. If the invoice is not payable, it returns a 409 Conflict error.
pub async fn get_paylink(
    State(state): State<Arc<AppState>>,
    Path(id): Path<u64>,
) -> Result<Json<PayLinkInfo>, EmeiError> {
    // Fetch invoice from chain
    let calldata = IEMEIInvoice::getInvoiceCall {
        invoiceId: U256::from(id),
    }
    .abi_encode();

    let result = state
        .chain
        .call(state.config.invoice_address, calldata.into())
        .await?;

    let invoice = IEMEIInvoice::getInvoiceCall::abi_decode_returns(&result)
        .map_err(|e| EmeiError::Internal(format!("failed to decode invoice: {e}")))?;

    // Check if invoice is in a payable state (PRESENTED or OVERDUE)
    let status_str = match invoice.status {
        0 => "ISSUED",
        1 => "PRESENTED",
        2 => "PAID",
        3 => "OVERDUE",
        4 => "REJECTED",
        _ => "UNKNOWN",
    };

    if invoice.status != 1 && invoice.status != 3 {
        return Err(EmeiError::Conflict(format!(
            "invoice {} is not payable (status: {})",
            id, status_str
        )));
    }

    // Generate approve calldata: ERC-20 approve(settlement_address, amount)
    let approve_calldata = {
        let mut data = vec![0x09, 0x5e, 0xa7, 0xb3]; // approve(address,uint256)
        data.extend_from_slice(&[0u8; 12]); // pad address to 32 bytes
        data.extend_from_slice(state.config.settlement_address.as_slice());
        data.extend_from_slice(&invoice.amount.to_be_bytes::<32>());
        format!("0x{}", hex::encode(&data))
    };

    // Generate pay calldata: EMEIInvoice.pay(invoiceId)
    let pay_calldata_bytes = IEMEIInvoice::payCall {
        invoiceId: U256::from(id),
    }
    .abi_encode();

    Ok(Json(PayLinkInfo {
        invoice_id: id,
        issuer: format!("0x{}", hex::encode(invoice.issuer)),
        payer: format!("0x{}", hex::encode(invoice.payer)),
        amount: invoice.amount.to_string(),
        asset: format!("0x{}", hex::encode(invoice.asset)),
        status: status_str.to_string(),
        settlement_contract: format!("0x{}", hex::encode(state.config.settlement_address)),
        invoice_contract: format!("0x{}", hex::encode(state.config.invoice_address)),
        approve_calldata,
        pay_calldata: format!("0x{}", hex::encode(&pay_calldata_bytes)),
        approve_to: format!("0x{}", hex::encode(invoice.asset)),
        pay_to: format!("0x{}", hex::encode(state.config.invoice_address)),
    }))
}
