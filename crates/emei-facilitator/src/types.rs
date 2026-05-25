use serde::{Deserialize, Serialize};

use crate::error::EmeiError;

// ─── Request Types ───────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct CreateInvoiceRequest {
    pub payer: String,
    pub amount: String,
    pub asset: String,
    pub line_items: Vec<LineItemRequest>,
    pub terms: TermsRequest,
    pub collection_mode: String, // "mandate" or "pay_link"
}

#[derive(Debug, Deserialize)]
pub struct LineItemRequest {
    pub description: String,
    pub amount: String,
    pub category: String,
}

#[derive(Debug, Deserialize)]
pub struct TermsRequest {
    pub term_type: String, // "due_on_receipt", "net_n_days", "milestones"
    pub net_days: Option<u64>,
    pub milestones: Option<Vec<MilestoneRequest>>,
}

#[derive(Debug, Deserialize)]
pub struct MilestoneRequest {
    pub amount: String,
    pub due_date: u64,
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct PresentRequest {
    pub invoice_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct PayRequest {
    pub invoice_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct CollectRequest {
    pub invoice_id: u64,
    pub mandate_id: u64,
}

#[derive(Debug, Deserialize)]
pub struct CreateMandateRequest {
    pub spend_cap: String,
    pub approved_counterparties: Vec<String>,
    pub approved_categories: Vec<String>,
    pub valid_from: u64,
    pub valid_until: u64,
}

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub initial_score: Option<u64>,
}

#[derive(Debug, Deserialize)]
pub struct WithdrawRequest {
    pub amount: String,
}

#[derive(Debug, Deserialize)]
pub struct StatementQueryParams {
    pub payer: String,
    pub status: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub offset: Option<u64>,
    pub limit: Option<u64>,
}

#[derive(Debug, Serialize)]
pub struct TxResponse {
    pub tx_hash: String,
}

#[derive(Debug, Serialize)]
pub struct InvoiceResponse {
    pub invoice_id: String,
    pub issuer: String,
    pub payer: String,
    pub amount: String,
    pub asset: String,
    pub status: String,
    pub collection_mode: String,
    pub settlement_proof: String,
    pub presented_at: String,
    pub created_at: String,
}

#[derive(Debug, Serialize)]
pub struct BalanceResponse {
    pub balance: String,
    pub accrued_yield: String,
}

#[derive(Debug, Serialize)]
pub struct ReputationResponse {
    pub address: String,
    pub score: u64,
}

#[derive(Debug, Serialize)]
pub struct VerifyResponse {
    pub verified: bool,
    pub batch_number: u64,
}

impl CreateInvoiceRequest {
    pub fn validate(&self) -> Result<(), EmeiError> {
        // amount must be non-zero numeric
        if self.amount == "0" || self.amount.parse::<u128>().is_err() {
            return Err(EmeiError::Validation {
                field: "amount".into(),
                reason: "must be a non-zero positive integer".into(),
            });
        }
        // payer must be valid address (40 hex chars)
        validate_address(&self.payer, "payer")?;
        validate_address(&self.asset, "asset")?;
        // line_items: 1..=50
        if self.line_items.is_empty() || self.line_items.len() > 50 {
            return Err(EmeiError::Validation {
                field: "line_items".into(),
                reason: "must have 1 to 50 items".into(),
            });
        }
        // terms validation
        match self.terms.term_type.as_str() {
            "due_on_receipt" => {}
            "net_n_days" => {
                let n = self.terms.net_days.unwrap_or(0);
                if n < 1 || n > 365 {
                    return Err(EmeiError::Validation {
                        field: "terms.net_days".into(),
                        reason: "must be between 1 and 365".into(),
                    });
                }
            }
            "milestones" => {
                let ms = self.terms.milestones.as_ref();
                if ms.is_none() || ms.unwrap().is_empty() || ms.unwrap().len() > 10 {
                    return Err(EmeiError::Validation {
                        field: "terms.milestones".into(),
                        reason: "must have 1 to 10 milestones".into(),
                    });
                }
            }
            _ => {
                return Err(EmeiError::Validation {
                    field: "terms.term_type".into(),
                    reason: "must be due_on_receipt, net_n_days, or milestones".into(),
                });
            }
        }
        Ok(())
    }
}

impl CreateMandateRequest {
    pub fn validate(&self) -> Result<(), EmeiError> {
        if self.spend_cap == "0" || self.spend_cap.parse::<u128>().is_err() {
            return Err(EmeiError::Validation {
                field: "spend_cap".into(),
                reason: "must be a non-zero positive integer".into(),
            });
        }
        if self.approved_counterparties.is_empty() || self.approved_counterparties.len() > 50 {
            return Err(EmeiError::Validation {
                field: "approved_counterparties".into(),
                reason: "must have 1 to 50 entries".into(),
            });
        }
        if self.approved_categories.is_empty() || self.approved_categories.len() > 20 {
            return Err(EmeiError::Validation {
                field: "approved_categories".into(),
                reason: "must have 1 to 20 entries".into(),
            });
        }
        if self.valid_until <= self.valid_from {
            return Err(EmeiError::Validation {
                field: "valid_until".into(),
                reason: "must be after valid_from".into(),
            });
        }
        for addr in &self.approved_counterparties {
            validate_address(addr, "approved_counterparties")?;
        }
        Ok(())
    }
}

/// Validate that a string is a valid Ethereum address (40 hex chars, with optional 0x prefix).
pub fn validate_address(s: &str, field: &str) -> Result<(), EmeiError> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    if stripped.len() != 40 || !stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(EmeiError::Validation {
            field: field.into(),
            reason: format!("invalid Ethereum address: {s}"),
        });
    }
    Ok(())
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_address() -> String {
        "0xC35f709255D7199394655F16008e8d1A3AD80005".to_string()
    }

    fn valid_invoice_request() -> CreateInvoiceRequest {
        CreateInvoiceRequest {
            payer: valid_address(),
            amount: "1000".to_string(),
            asset: valid_address(),
            line_items: vec![LineItemRequest {
                description: "Service fee".to_string(),
                amount: "1000".to_string(),
                category: "services".to_string(),
            }],
            terms: TermsRequest {
                term_type: "due_on_receipt".to_string(),
                net_days: None,
                milestones: None,
            },
            collection_mode: "pay_link".to_string(),
        }
    }

    fn valid_mandate_request() -> CreateMandateRequest {
        CreateMandateRequest {
            spend_cap: "5000".to_string(),
            approved_counterparties: vec![valid_address()],
            approved_categories: vec!["services".to_string()],
            valid_from: 1000,
            valid_until: 2000,
        }
    }

    #[test]
    fn test_valid_invoice_request_passes() {
        let req = valid_invoice_request();
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_invoice_zero_amount_fails() {
        let mut req = valid_invoice_request();
        req.amount = "0".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "amount"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_non_numeric_amount_fails() {
        let mut req = valid_invoice_request();
        req.amount = "abc".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "amount"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_empty_line_items_fails() {
        let mut req = valid_invoice_request();
        req.line_items = vec![];
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "line_items"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_over_50_line_items_fails() {
        let mut req = valid_invoice_request();
        req.line_items = (0..51)
            .map(|i| LineItemRequest {
                description: format!("item {i}"),
                amount: "10".to_string(),
                category: "misc".to_string(),
            })
            .collect();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "line_items"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_invalid_payer_address_fails() {
        let mut req = valid_invoice_request();
        req.payer = "not_an_address".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "payer"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_invalid_asset_address_fails() {
        let mut req = valid_invoice_request();
        req.asset = "0x123".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "asset"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_invalid_term_type_fails() {
        let mut req = valid_invoice_request();
        req.terms.term_type = "invalid".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "terms.term_type"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_net_n_days_zero_fails() {
        let mut req = valid_invoice_request();
        req.terms.term_type = "net_n_days".to_string();
        req.terms.net_days = Some(0);
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "terms.net_days"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_invoice_net_n_days_over_365_fails() {
        let mut req = valid_invoice_request();
        req.terms.term_type = "net_n_days".to_string();
        req.terms.net_days = Some(366);
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "terms.net_days"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_valid_mandate_request_passes() {
        let req = valid_mandate_request();
        assert!(req.validate().is_ok());
    }

    #[test]
    fn test_mandate_zero_spend_cap_fails() {
        let mut req = valid_mandate_request();
        req.spend_cap = "0".to_string();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "spend_cap"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_over_50_counterparties_fails() {
        let mut req = valid_mandate_request();
        req.approved_counterparties = (0..51).map(|_| valid_address()).collect();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "approved_counterparties"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_empty_counterparties_fails() {
        let mut req = valid_mandate_request();
        req.approved_counterparties = vec![];
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "approved_counterparties"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_over_20_categories_fails() {
        let mut req = valid_mandate_request();
        req.approved_categories = (0..21).map(|i| format!("cat_{i}")).collect();
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "approved_categories"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_valid_until_equals_valid_from_fails() {
        let mut req = valid_mandate_request();
        req.valid_until = req.valid_from;
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "valid_until"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_valid_until_before_valid_from_fails() {
        let mut req = valid_mandate_request();
        req.valid_until = req.valid_from - 1;
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "valid_until"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_mandate_invalid_counterparty_address_fails() {
        let mut req = valid_mandate_request();
        req.approved_counterparties = vec!["0xINVALID".to_string()];
        let err = req.validate().unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "approved_counterparties"),
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_validate_address_valid_with_prefix() {
        assert!(validate_address("0xC35f709255D7199394655F16008e8d1A3AD80005", "test").is_ok());
    }

    #[test]
    fn test_validate_address_valid_without_prefix() {
        assert!(validate_address("C35f709255D7199394655F16008e8d1A3AD80005", "test").is_ok());
    }

    #[test]
    fn test_validate_address_too_short() {
        let err = validate_address("0x1234", "test").unwrap_err();
        match err {
            EmeiError::Validation { field, reason } => {
                assert_eq!(field, "test");
                assert!(reason.contains("invalid Ethereum address"));
            }
            _ => panic!("expected Validation error"),
        }
    }

    #[test]
    fn test_validate_address_non_hex() {
        let err =
            validate_address("0xGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG", "test").unwrap_err();
        match err {
            EmeiError::Validation { field, .. } => assert_eq!(field, "test"),
            _ => panic!("expected Validation error"),
        }
    }
}
