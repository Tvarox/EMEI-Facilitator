//! Mandate route handlers: POST /mandate, DELETE /mandate/:id

use std::sync::Arc;

use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{
    contracts::mandate::IEMEIMandate, error::EmeiError, signing::UserSigner, state::AppState,
    types::*,
};

/// POST /emei/mandate — Create a new spending mandate.
pub async fn create_mandate(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<CreateMandateRequest>,
) -> Result<(StatusCode, Json<TxResponse>), EmeiError> {
    body.validate()?;

    let spend_cap = body
        .spend_cap
        .parse::<U256>()
        .map_err(|_| EmeiError::Validation {
            field: "spend_cap".into(),
            reason: "invalid U256".into(),
        })?;

    let approved_counterparties: Result<Vec<Address>, EmeiError> = body
        .approved_counterparties
        .iter()
        .map(|a| {
            a.parse::<Address>().map_err(|_| EmeiError::Validation {
                field: "approved_counterparties".into(),
                reason: format!("invalid address: {a}"),
            })
        })
        .collect();
    let approved_counterparties = approved_counterparties?;

    let params = IEMEIMandate::CreateMandateParams {
        spendCap: spend_cap,
        approvedCounterparties: approved_counterparties,
        approvedCategories: body.approved_categories.clone(),
        validFrom: U256::from(body.valid_from),
        validUntil: U256::from(body.valid_until),
    };

    let calldata = IEMEIMandate::createMandateCall { params }.abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.mandate_address, calldata.into())
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(TxResponse {
            tx_hash: format!("0x{}", hex::encode(tx_hash)),
        }),
    ))
}

/// DELETE /emei/mandate/:id — Revoke a mandate.
pub async fn revoke_mandate(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Path(id): Path<u64>,
) -> Result<Json<TxResponse>, EmeiError> {
    let calldata = IEMEIMandate::revokeMandateCall {
        mandateId: U256::from(id),
    }
    .abi_encode();

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.mandate_address, calldata.into())
        .await?;

    Ok(Json(TxResponse {
        tx_hash: format!("0x{}", hex::encode(tx_hash)),
    }))
}
