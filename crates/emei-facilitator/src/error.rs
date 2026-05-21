/// Error type for the EMEI facilitator backend.
///
/// Implements `IntoResponse` for Axum, mapping each variant
/// to the appropriate HTTP status code and JSON error body.
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};

use crate::contracts::{
    erc8004::IMockERC8004, invoice::IEMEIInvoice, mandate::IEMEIMandate,
    settlement::IEMEISettlement,
};
use alloy_sol_types::SolError;

#[derive(Debug, thiserror::Error)]
pub enum EmeiError {
    #[error("validation error: {field}: {reason}")]
    Validation { field: String, reason: String },

    #[error("missing authentication")]
    MissingAuth,

    #[error("invalid authentication: {0}")]
    InvalidAuth(String),

    #[error("insufficient funds: {0}")]
    InsufficientFunds(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("not found: {resource}")]
    NotFound { resource: String },

    #[error("conflict: {0}")]
    Conflict(String),

    #[error("business logic error: {0}")]
    BusinessLogic(String),

    #[error("contract revert: {name}({params})")]
    ContractRevert {
        name: String,
        params: String,
        status: StatusCode,
    },

    #[error("rpc error: {0}")]
    RpcError(String),

    #[error("rpc timeout")]
    RpcTimeout,

    #[error("internal error: {0}")]
    Internal(String),

    #[error("database error: {0}")]
    Database(String),
}

#[derive(serde::Serialize)]
pub struct ErrorResponse {
    pub error_code: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
}

impl IntoResponse for EmeiError {
    fn into_response(self) -> Response {
        let (status, error_code, resource) = match &self {
            EmeiError::Validation { field, .. } => (
                StatusCode::BAD_REQUEST,
                "VALIDATION_ERROR",
                Some(field.clone()),
            ),
            EmeiError::MissingAuth => (StatusCode::UNAUTHORIZED, "MISSING_AUTH", None),
            EmeiError::InvalidAuth(_) => (StatusCode::UNAUTHORIZED, "INVALID_AUTH", None),
            EmeiError::InsufficientFunds(_) => {
                (StatusCode::PAYMENT_REQUIRED, "INSUFFICIENT_FUNDS", None)
            }
            EmeiError::Unauthorized(_) => (StatusCode::FORBIDDEN, "UNAUTHORIZED", None),
            EmeiError::NotFound { resource } => {
                (StatusCode::NOT_FOUND, "NOT_FOUND", Some(resource.clone()))
            }
            EmeiError::Conflict(_) => (StatusCode::CONFLICT, "STATE_CONFLICT", None),
            EmeiError::BusinessLogic(_) => (
                StatusCode::UNPROCESSABLE_ENTITY,
                "BUSINESS_LOGIC_ERROR",
                None,
            ),
            EmeiError::ContractRevert { status, .. } => (*status, "CONTRACT_REVERT", None),
            EmeiError::RpcError(_) => (StatusCode::BAD_GATEWAY, "RPC_ERROR", None),
            EmeiError::RpcTimeout => (StatusCode::GATEWAY_TIMEOUT, "RPC_TIMEOUT", None),
            EmeiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "INTERNAL_ERROR", None),
            EmeiError::Database(_) => (StatusCode::INTERNAL_SERVER_ERROR, "DATABASE_ERROR", None),
        };

        let body = ErrorResponse {
            error_code: error_code.to_string(),
            message: self.to_string(),
            resource,
        };

        (status, Json(body)).into_response()
    }
}

/// Attempt to decode raw revert data from a contract call into a
/// structured `EmeiError`. Checks known error selectors from the
/// Invoice, Mandate, and Settlement contracts.
///
/// If the selector doesn't match any known error, returns a generic
/// `ContractRevert` with hex-encoded data.
pub fn decode_revert(data: &[u8]) -> EmeiError {
    if data.len() < 4 {
        return EmeiError::ContractRevert {
            name: "Unknown".into(),
            params: hex::encode(data),
            status: StatusCode::INTERNAL_SERVER_ERROR,
        };
    }

    let selector: [u8; 4] = data[..4].try_into().unwrap();

    // --- IEMEIInvoice errors ---

    // Unauthorized()
    if selector == IEMEIInvoice::Unauthorized::SELECTOR {
        return EmeiError::Unauthorized("contract: Unauthorized".into());
    }

    // InvoiceNotFound(uint256)
    if selector == IEMEIInvoice::InvoiceNotFound::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::InvoiceNotFound::abi_decode(data) {
            return EmeiError::NotFound {
                resource: format!("invoice:{}", decoded.invoiceId),
            };
        }
        return EmeiError::NotFound {
            resource: "invoice".into(),
        };
    }

    // InvalidInvoiceParams(string)
    if selector == IEMEIInvoice::InvalidInvoiceParams::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::InvalidInvoiceParams::abi_decode(data) {
            return EmeiError::Validation {
                field: "invoice_params".into(),
                reason: decoded.reason,
            };
        }
        return EmeiError::Validation {
            field: "invoice_params".into(),
            reason: "invalid parameters".into(),
        };
    }

    // ReputationTooLow(address,uint256,uint256)
    if selector == IEMEIInvoice::ReputationTooLow::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::ReputationTooLow::abi_decode(data) {
            return EmeiError::Unauthorized(format!(
                "ReputationTooLow: account={}, score={}, threshold={}",
                decoded.account, decoded.score, decoded.threshold
            ));
        }
        return EmeiError::Unauthorized("ReputationTooLow".into());
    }

    // InvalidStatusTransition(uint8,uint8)
    if selector == IEMEIInvoice::InvalidStatusTransition::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::InvalidStatusTransition::abi_decode(data) {
            return EmeiError::Conflict(format!(
                "InvalidStatusTransition: current={}, target={}",
                decoded.current, decoded.target
            ));
        }
        return EmeiError::Conflict("InvalidStatusTransition".into());
    }

    // AmountMismatch(uint256,uint256)
    if selector == IEMEIInvoice::AmountMismatch::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::AmountMismatch::abi_decode(data) {
            return EmeiError::Validation {
                field: "amount".into(),
                reason: format!(
                    "AmountMismatch: expected={}, actual={}",
                    decoded.expected, decoded.actual
                ),
            };
        }
        return EmeiError::Validation {
            field: "amount".into(),
            reason: "AmountMismatch".into(),
        };
    }

    // SettlementFailed(string)
    if selector == IEMEIInvoice::SettlementFailed::SELECTOR {
        if let Ok(decoded) = IEMEIInvoice::SettlementFailed::abi_decode(data) {
            return EmeiError::BusinessLogic(format!("SettlementFailed: {}", decoded.reason));
        }
        return EmeiError::BusinessLogic("SettlementFailed".into());
    }

    // --- IEMEIMandate errors ---

    // InvalidMandateParams(string)
    if selector == IEMEIMandate::InvalidMandateParams::SELECTOR {
        if let Ok(decoded) = IEMEIMandate::InvalidMandateParams::abi_decode(data) {
            return EmeiError::Validation {
                field: "mandate_params".into(),
                reason: decoded.reason,
            };
        }
        return EmeiError::Validation {
            field: "mandate_params".into(),
            reason: "invalid mandate parameters".into(),
        };
    }

    // MandateNotFound(uint256)
    if selector == IEMEIMandate::MandateNotFound::SELECTOR {
        if let Ok(decoded) = IEMEIMandate::MandateNotFound::abi_decode(data) {
            return EmeiError::NotFound {
                resource: format!("mandate:{}", decoded.mandateId),
            };
        }
        return EmeiError::NotFound {
            resource: "mandate".into(),
        };
    }

    // MandateExpired(uint256)
    if selector == IEMEIMandate::MandateExpired::SELECTOR {
        if let Ok(decoded) = IEMEIMandate::MandateExpired::abi_decode(data) {
            return EmeiError::BusinessLogic(format!(
                "MandateExpired: mandateId={}",
                decoded.mandateId
            ));
        }
        return EmeiError::BusinessLogic("MandateExpired".into());
    }

    // InsufficientMandateCap(uint256,uint256)
    if selector == IEMEIMandate::InsufficientMandateCap::SELECTOR {
        if let Ok(decoded) = IEMEIMandate::InsufficientMandateCap::abi_decode(data) {
            return EmeiError::BusinessLogic(format!(
                "InsufficientMandateCap: remaining={}, required={}",
                decoded.remaining, decoded.required
            ));
        }
        return EmeiError::BusinessLogic("InsufficientMandateCap".into());
    }

    // MandateNotActive(uint256)
    if selector == IEMEIMandate::MandateNotActive::SELECTOR {
        if let Ok(decoded) = IEMEIMandate::MandateNotActive::abi_decode(data) {
            return EmeiError::BusinessLogic(format!(
                "MandateNotActive: mandateId={}",
                decoded.mandateId
            ));
        }
        return EmeiError::BusinessLogic("MandateNotActive".into());
    }

    // Mandate Unauthorized() - same selector as Invoice Unauthorized, already handled above

    // --- IEMEISettlement errors ---

    // AlreadyRegistered(address) from MockERC8004
    if selector == IMockERC8004::AlreadyRegistered::SELECTOR {
        if let Ok(decoded) = IMockERC8004::AlreadyRegistered::abi_decode(data) {
            return EmeiError::Conflict(format!(
                "AlreadyRegistered: address {} is already registered in the identity registry",
                decoded.agent
            ));
        }
        return EmeiError::Conflict("AlreadyRegistered: this address is already registered".into());
    }

    // TransferFailed(address,address,address,uint256)
    if selector == IEMEISettlement::TransferFailed::SELECTOR {
        if let Ok(decoded) = IEMEISettlement::TransferFailed::abi_decode(data) {
            return EmeiError::InsufficientFunds(format!(
                "TransferFailed: token={}, from={}, to={}, amount={}",
                decoded.token, decoded.from, decoded.to, decoded.amount
            ));
        }
        return EmeiError::InsufficientFunds("TransferFailed".into());
    }

    // InsufficientVaultBalance(address,uint256,uint256)
    if selector == IEMEISettlement::InsufficientVaultBalance::SELECTOR {
        if let Ok(decoded) = IEMEISettlement::InsufficientVaultBalance::abi_decode(data) {
            return EmeiError::InsufficientFunds(format!(
                "InsufficientVaultBalance: payee={}, requested={}, available={}",
                decoded.payee, decoded.requested, decoded.available
            ));
        }
        return EmeiError::InsufficientFunds("InsufficientVaultBalance".into());
    }

    // SwapFailed(uint256,uint256)
    if selector == IEMEISettlement::SwapFailed::SELECTOR {
        if let Ok(decoded) = IEMEISettlement::SwapFailed::abi_decode(data) {
            return EmeiError::BusinessLogic(format!(
                "SwapFailed: expected={}, received={}",
                decoded.expected, decoded.received
            ));
        }
        return EmeiError::BusinessLogic("SwapFailed".into());
    }

    // Fallback: unknown selector
    EmeiError::ContractRevert {
        name: "Unknown".into(),
        params: hex::encode(data),
        status: StatusCode::INTERNAL_SERVER_ERROR,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::{Address, U256};
    use alloy_sol_types::SolError;
    use http_body_util::BodyExt;

    #[test]
    fn test_decode_revert_unauthorized() {
        let data = IEMEIInvoice::Unauthorized {}.abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::Unauthorized(msg) => assert!(msg.contains("Unauthorized")),
            other => panic!("expected Unauthorized, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_invoice_not_found() {
        let data = IEMEIInvoice::InvoiceNotFound {
            invoiceId: U256::from(42),
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::NotFound { resource } => assert!(resource.contains("42")),
            other => panic!("expected NotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_invalid_invoice_params() {
        let data = IEMEIInvoice::InvalidInvoiceParams {
            reason: "bad amount".into(),
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::Validation { field, reason } => {
                assert_eq!(field, "invoice_params");
                assert!(reason.contains("bad amount"));
            }
            other => panic!("expected Validation, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_reputation_too_low() {
        let data = IEMEIInvoice::ReputationTooLow {
            account: Address::ZERO,
            score: U256::from(10),
            threshold: U256::from(100),
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::Unauthorized(msg) => {
                assert!(msg.contains("ReputationTooLow"));
                assert!(msg.contains("10"));
                assert!(msg.contains("100"));
            }
            other => panic!("expected Unauthorized, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_invalid_status_transition() {
        let data = IEMEIInvoice::InvalidStatusTransition {
            current: 1,
            target: 3,
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::Conflict(msg) => {
                assert!(msg.contains("InvalidStatusTransition"));
            }
            other => panic!("expected Conflict, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_mandate_not_found() {
        let data = IEMEIMandate::MandateNotFound {
            mandateId: U256::from(7),
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::NotFound { resource } => assert!(resource.contains("7")),
            other => panic!("expected NotFound, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_transfer_failed() {
        let data = IEMEISettlement::TransferFailed {
            token: Address::ZERO,
            from: Address::ZERO,
            to: Address::ZERO,
            amount: U256::from(1000),
        }
        .abi_encode();
        let err = decode_revert(&data);
        match err {
            EmeiError::InsufficientFunds(msg) => {
                assert!(msg.contains("TransferFailed"));
                assert!(msg.contains("1000"));
            }
            other => panic!("expected InsufficientFunds, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_unknown_selector() {
        let data = vec![0xde, 0xad, 0xbe, 0xef, 0x01, 0x02];
        let err = decode_revert(&data);
        match err {
            EmeiError::ContractRevert {
                name,
                params,
                status,
            } => {
                assert_eq!(name, "Unknown");
                assert_eq!(params, "deadbeef0102");
                assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
            }
            other => panic!("expected ContractRevert, got: {:?}", other),
        }
    }

    #[test]
    fn test_decode_revert_short_data() {
        let data = vec![0x01, 0x02];
        let err = decode_revert(&data);
        match err {
            EmeiError::ContractRevert { name, .. } => assert_eq!(name, "Unknown"),
            other => panic!("expected ContractRevert, got: {:?}", other),
        }
    }

    #[tokio::test]
    async fn test_into_response_validation() {
        let err = EmeiError::Validation {
            field: "amount".into(),
            reason: "must be non-zero".into(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error_code"], "VALIDATION_ERROR");
        assert_eq!(json["resource"], "amount");
    }

    #[tokio::test]
    async fn test_into_response_not_found() {
        let err = EmeiError::NotFound {
            resource: "invoice:42".into(),
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error_code"], "NOT_FOUND");
        assert_eq!(json["resource"], "invoice:42");
    }

    #[tokio::test]
    async fn test_into_response_contract_revert() {
        let err = EmeiError::ContractRevert {
            name: "InvoiceNotFound".into(),
            params: "invoiceId=42".into(),
            status: StatusCode::NOT_FOUND,
        };
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);

        let body = response.into_body().collect().await.unwrap().to_bytes();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["error_code"], "CONTRACT_REVERT");
        assert!(
            json["message"]
                .as_str()
                .unwrap()
                .contains("InvoiceNotFound")
        );
    }

    #[tokio::test]
    async fn test_into_response_missing_auth() {
        let err = EmeiError::MissingAuth;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_into_response_rpc_error() {
        let err = EmeiError::RpcError("connection refused".into());
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn test_into_response_rpc_timeout() {
        let err = EmeiError::RpcTimeout;
        let response = err.into_response();
        assert_eq!(response.status(), StatusCode::GATEWAY_TIMEOUT);
    }
}
