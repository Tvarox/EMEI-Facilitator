//! Identity route handler: POST /register

use std::sync::Arc;

use alloy_primitives::U256;
use alloy_sol_types::SolCall;
use axum::{Json, extract::State, http::StatusCode};

use crate::{
    contracts::erc8004::IMockERC8004, error::EmeiError, signing::UserSigner, state::AppState,
    types::*,
};

/// POST /emei/register — Register an identity in the ERC-8004 registry.
pub async fn register_identity(
    State(state): State<Arc<AppState>>,
    signer: UserSigner,
    Json(body): Json<RegisterRequest>,
) -> Result<(StatusCode, Json<TxResponse>), EmeiError> {
    let calldata = match body.initial_score {
        Some(score) => IMockERC8004::register_1Call {
            initialScore: U256::from(score),
        }
        .abi_encode(),
        None => IMockERC8004::register_0Call {}.abi_encode(),
    };

    let tx_hash = state
        .chain
        .send_user(signer.0, state.config.erc8004_address, calldata.into())
        .await?;

    Ok((
        StatusCode::CREATED,
        Json(TxResponse {
            tx_hash: format!("0x{}", hex::encode(tx_hash)),
        }),
    ))
}
