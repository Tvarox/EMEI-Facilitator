//! Withdrawal route handler: POST /withdraw

use std::sync::Arc;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use axum::{Json, extract::State};

use crate::{
    contracts::settlement::IEMEISettlement, error::EmeiError, signing::UserSigner, state::AppState,
    types::*,
};

/// POST /emei/withdraw — Withdraw funds from the settlement vault.
pub async fn withdraw_funds(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<WithdrawRequest>,
) -> Result<Json<TxResponse>, EmeiError> {
    let amount = body
        .amount
        .parse::<U256>()
        .map_err(|_| EmeiError::Validation {
            field: "amount".into(),
            reason: "invalid U256".into(),
        })?;

    if amount == U256::ZERO {
        return Err(EmeiError::Validation {
            field: "amount".into(),
            reason: "must be non-zero".into(),
        });
    }

    let calldata = IEMEISettlement::withdrawCall { amount }.abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.settlement_address, calldata.into())
        .await?;

    Ok(Json(TxResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    }))
}
