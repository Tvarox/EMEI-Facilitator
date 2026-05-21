use std::fmt;
use std::str::FromStr;

use alloy_primitives::{Address, B256};

/// Errors that can occur during configuration loading and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigError {
    /// A required environment variable is missing.
    MissingVar(String),
    /// An address value could not be parsed as a valid 20-byte Ethereum address.
    InvalidAddress(String),
    /// A hex key value could not be parsed as a valid 32-byte key.
    InvalidKey(String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::MissingVar(name) => {
                write!(f, "missing required environment variable: {name}")
            }
            ConfigError::InvalidAddress(detail) => {
                write!(f, "invalid Ethereum address: {detail}")
            }
            ConfigError::InvalidKey(detail) => {
                write!(f, "invalid hex key: {detail}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Configuration for the EMEI facilitator backend.
///
/// Loaded from environment variables via `EmeiConfig::from_env()`.
#[derive(Debug, Clone)]
pub struct EmeiConfig {
    /// RPC endpoint URL for Mantle Sepolia.
    pub rpc_url: String,

    /// Address of the EMEIInvoice contract.
    pub invoice_address: Address,
    /// Address of the EMEIMandate contract.
    pub mandate_address: Address,
    /// Address of the EMEISettlement contract.
    pub settlement_address: Address,
    /// Address of the EMEIReceipt contract.
    pub receipt_address: Address,
    /// Address of the Bay8004 reputation contract.
    pub bay8004_address: Address,
    /// Address of the MockERC8004 identity registry contract.
    pub erc8004_address: Address,

    /// Private key for the hot wallet used by background services.
    pub hot_wallet_key: B256,

    /// Path to the SQLite database file.
    pub sqlite_path: String,

    /// Interval in seconds between receipt batching cycles.
    pub batch_interval: u64,
    /// Interval in seconds between auto-collection cycles.
    pub collect_interval: u64,
    /// Interval in seconds between overdue scanning cycles.
    pub overdue_interval: u64,
}

impl EmeiConfig {
    /// Load configuration from environment variables.
    ///
    /// Required variables:
    /// - `EMEI_RPC_URL`
    /// - `EMEI_HOT_WALLET_KEY`
    /// - `EMEI_INVOICE_ADDRESS`
    /// - `EMEI_MANDATE_ADDRESS`
    /// - `EMEI_SETTLEMENT_ADDRESS`
    /// - `EMEI_RECEIPT_ADDRESS`
    /// - `EMEI_BAY8004_ADDRESS`
    /// - `EMEI_ERC8004_ADDRESS`
    ///
    /// Optional variables (with defaults):
    /// - `EMEI_SQLITE_PATH` (default: `"./emei.db"`)
    /// - `EMEI_BATCH_INTERVAL` (default: `30`)
    /// - `EMEI_COLLECT_INTERVAL` (default: `10`)
    /// - `EMEI_OVERDUE_INTERVAL` (default: `60`)
    pub fn from_env() -> Result<Self, ConfigError> {
        let rpc_url = env_required("EMEI_RPC_URL")?;
        let hot_wallet_key = parse_hex_key(&env_required("EMEI_HOT_WALLET_KEY")?)?;

        let invoice_address = parse_address(&env_required("EMEI_INVOICE_ADDRESS")?)?;
        let mandate_address = parse_address(&env_required("EMEI_MANDATE_ADDRESS")?)?;
        let settlement_address = parse_address(&env_required("EMEI_SETTLEMENT_ADDRESS")?)?;
        let receipt_address = parse_address(&env_required("EMEI_RECEIPT_ADDRESS")?)?;
        let bay8004_address = parse_address(&env_required("EMEI_BAY8004_ADDRESS")?)?;
        let erc8004_address = parse_address(&env_required("EMEI_ERC8004_ADDRESS")?)?;

        let sqlite_path = env_or_default("EMEI_SQLITE_PATH", "./emei.db".to_string());
        let batch_interval = env_or_default("EMEI_BATCH_INTERVAL", 30u64);
        let collect_interval = env_or_default("EMEI_COLLECT_INTERVAL", 10u64);
        let overdue_interval = env_or_default("EMEI_OVERDUE_INTERVAL", 60u64);

        Ok(Self {
            rpc_url,
            invoice_address,
            mandate_address,
            settlement_address,
            receipt_address,
            bay8004_address,
            erc8004_address,
            hot_wallet_key,
            sqlite_path,
            batch_interval,
            collect_interval,
            overdue_interval,
        })
    }
}

/// Read a required environment variable, returning `ConfigError::MissingVar` if absent.
pub fn env_required(name: &str) -> Result<String, ConfigError> {
    std::env::var(name).map_err(|_| ConfigError::MissingVar(name.to_string()))
}

/// Read an optional environment variable, returning `default` if absent or unparseable.
pub fn env_or_default<T: FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse::<T>().ok())
        .unwrap_or(default)
}

/// Parse a hex string as a valid 20-byte Ethereum address.
///
/// Accepts both `0x`-prefixed and bare hex strings.
pub fn parse_address(s: &str) -> Result<Address, ConfigError> {
    s.parse::<Address>()
        .map_err(|_| ConfigError::InvalidAddress(s.to_string()))
}

/// Parse a hex string as a valid 32-byte key (B256).
///
/// Accepts both `0x`-prefixed and bare hex strings.
pub fn parse_hex_key(s: &str) -> Result<B256, ConfigError> {
    let stripped = s.strip_prefix("0x").unwrap_or(s);
    let bytes = hex::decode(stripped).map_err(|_| ConfigError::InvalidKey(s.to_string()))?;
    if bytes.len() != 32 {
        return Err(ConfigError::InvalidKey(format!(
            "expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(B256::from_slice(&bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_address_valid() {
        let addr = parse_address("0xC35f709255D7199394655F16008e8d1A3AD80005").unwrap();
        assert_eq!(
            addr,
            "0xC35f709255D7199394655F16008e8d1A3AD80005"
                .parse::<Address>()
                .unwrap()
        );
    }

    #[test]
    fn test_parse_address_no_prefix() {
        let addr = parse_address("C35f709255D7199394655F16008e8d1A3AD80005").unwrap();
        assert_eq!(
            addr,
            "0xC35f709255D7199394655F16008e8d1A3AD80005"
                .parse::<Address>()
                .unwrap()
        );
    }

    #[test]
    fn test_parse_address_invalid() {
        assert!(parse_address("not_an_address").is_err());
        assert!(parse_address("0x123").is_err());
        assert!(parse_address("").is_err());
    }

    #[test]
    fn test_parse_hex_key_valid() {
        let key_hex = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let key = parse_hex_key(key_hex).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_parse_hex_key_no_prefix() {
        let key_hex = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";
        let key = parse_hex_key(key_hex).unwrap();
        assert_eq!(key.len(), 32);
    }

    #[test]
    fn test_parse_hex_key_invalid() {
        assert!(parse_hex_key("not_hex").is_err());
        assert!(parse_hex_key("0x1234").is_err()); // too short
        assert!(parse_hex_key("").is_err());
    }

    #[test]
    fn test_env_required_missing() {
        // Use a variable name that definitely doesn't exist
        let result = env_required("EMEI_TEST_NONEXISTENT_VAR_12345");
        assert!(result.is_err());
        match result.unwrap_err() {
            ConfigError::MissingVar(name) => {
                assert_eq!(name, "EMEI_TEST_NONEXISTENT_VAR_12345");
            }
            _ => panic!("expected MissingVar"),
        }
    }

    #[test]
    fn test_env_or_default_uses_default() {
        let val: u64 = env_or_default("EMEI_TEST_NONEXISTENT_VAR_67890", 42);
        assert_eq!(val, 42);
    }

    #[test]
    fn test_config_error_display() {
        let err = ConfigError::MissingVar("EMEI_RPC_URL".to_string());
        assert_eq!(
            err.to_string(),
            "missing required environment variable: EMEI_RPC_URL"
        );

        let err = ConfigError::InvalidAddress("bad_addr".to_string());
        assert_eq!(err.to_string(), "invalid Ethereum address: bad_addr");

        let err = ConfigError::InvalidKey("bad_key".to_string());
        assert_eq!(err.to_string(), "invalid hex key: bad_key");
    }

    #[test]
    fn test_from_env_missing_rpc_url() {
        // Clear all EMEI vars to ensure clean state
        // SAFETY: This test is run single-threaded and no other threads
        // depend on these environment variables.
        unsafe {
            for key in &[
                "EMEI_RPC_URL",
                "EMEI_HOT_WALLET_KEY",
                "EMEI_INVOICE_ADDRESS",
                "EMEI_MANDATE_ADDRESS",
                "EMEI_SETTLEMENT_ADDRESS",
                "EMEI_RECEIPT_ADDRESS",
                "EMEI_BAY8004_ADDRESS",
                "EMEI_ERC8004_ADDRESS",
            ] {
                std::env::remove_var(key);
            }
        }

        let result = EmeiConfig::from_env();
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err(),
            ConfigError::MissingVar("EMEI_RPC_URL".to_string())
        );
    }
}
