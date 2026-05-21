/// Trait abstracting on-chain interactions.
///
/// Enables mocking for tests while providing a real alloy-rs
/// implementation for production use.
use alloy_network::{Ethereum, EthereumWallet, TransactionBuilder};
use alloy_primitives::{Address, B256, Bytes};
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport_http::reqwest;

use crate::error::{EmeiError, decode_revert};

#[async_trait::async_trait]
pub trait ChainClient: Send + Sync + 'static {
    /// Submit a transaction using the hot wallet signer.
    async fn send_hot(&self, to: Address, calldata: Bytes) -> Result<B256, EmeiError>;

    /// Submit a transaction using a user-provided signer.
    async fn send_user(
        &self,
        signer: PrivateKeySigner,
        to: Address,
        calldata: Bytes,
    ) -> Result<B256, EmeiError>;

    /// Execute a read-only call (no signing required).
    async fn call(&self, to: Address, calldata: Bytes) -> Result<Bytes, EmeiError>;
}

/// Production chain client using alloy-rs HTTP provider.
pub struct AlloyChainClient {
    provider: RootProvider<Ethereum>,
    hot_wallet: PrivateKeySigner,
}

impl AlloyChainClient {
    /// Create a new `AlloyChainClient` from an RPC URL and hot wallet private key.
    pub fn new(rpc_url: &str, hot_wallet_key: B256) -> Result<Self, EmeiError> {
        let url: reqwest::Url = rpc_url
            .parse()
            .map_err(|e| EmeiError::Internal(format!("invalid RPC URL: {e}")))?;

        // Use default() (no fillers) to get a bare RootProvider.
        // We add wallet fillers per-request in send_with_wallet.
        let provider = ProviderBuilder::default().connect_http(url);

        let hot_wallet = PrivateKeySigner::from_bytes(&hot_wallet_key)
            .map_err(|e| EmeiError::Internal(format!("invalid hot wallet key: {e}")))?;

        Ok(Self {
            provider,
            hot_wallet,
        })
    }

    /// Send a transaction signed by the given wallet, with a 5-second timeout.
    async fn send_with_wallet(
        &self,
        wallet: EthereumWallet,
        to: Address,
        calldata: Bytes,
    ) -> Result<B256, EmeiError> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata);

        // Build a provider with wallet + recommended fillers (gas, nonce, chain-id)
        // on top of our base provider.
        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_provider(&self.provider);

        // Send the transaction (signs, fills gas/nonce/chain-id, and submits)
        let pending = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            provider.send_transaction(tx),
        )
        .await
        .map_err(|_| EmeiError::RpcTimeout)?
        .map_err(|e| {
            let err_str = e.to_string();
            // Check for common RPC errors and provide clear messages
            if err_str.contains("insufficient funds") {
                // Extract balance and cost from the error message if available
                let detail = if let (Some(bal_idx), Some(cost_idx)) = (err_str.find("balance "), err_str.find("tx cost ")) {
                    let balance = err_str[bal_idx + 8..].split(|c: char| !c.is_ascii_digit()).next().unwrap_or("?");
                    let cost = err_str[cost_idx + 8..].split(|c: char| !c.is_ascii_digit()).next().unwrap_or("?");
                    format!(
                        "Insufficient MNT for gas. Account balance: {} wei, transaction cost: {} wei. Fund your account on Mantle Sepolia.",
                        balance, cost
                    )
                } else {
                    "Account does not have enough MNT to pay for gas. Please fund your account with MNT on Mantle Sepolia.".into()
                };
                return EmeiError::InsufficientFunds(detail);
            }
            if err_str.contains("nonce too low") {
                return EmeiError::Conflict(
                    "Transaction nonce conflict. A previous transaction may still be pending. Please wait and retry.".into()
                );
            }
            if let Some(data) = extract_revert_data(&err_str) {
                decode_revert(&data)
            } else {
                EmeiError::RpcError(err_str)
            }
        })?;

        // Return the tx hash immediately after submission (fire-and-forget).
        // Don't wait for block confirmation — Mantle Sepolia can be slow.
        Ok(*pending.tx_hash())
    }
}

#[async_trait::async_trait]
impl ChainClient for AlloyChainClient {
    async fn send_hot(&self, to: Address, calldata: Bytes) -> Result<B256, EmeiError> {
        let wallet = EthereumWallet::from(self.hot_wallet.clone());
        self.send_with_wallet(wallet, to, calldata).await
    }

    async fn send_user(
        &self,
        signer: PrivateKeySigner,
        to: Address,
        calldata: Bytes,
    ) -> Result<B256, EmeiError> {
        let wallet = EthereumWallet::from(signer);
        self.send_with_wallet(wallet, to, calldata).await
    }

    async fn call(&self, to: Address, calldata: Bytes) -> Result<Bytes, EmeiError> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata);

        let result =
            tokio::time::timeout(std::time::Duration::from_secs(15), self.provider.call(tx)).await;

        match result {
            Ok(Ok(bytes)) => Ok(bytes),
            Ok(Err(e)) => {
                let err_str = e.to_string();
                if let Some(data) = extract_revert_data(&err_str) {
                    Err(decode_revert(&data))
                } else {
                    Err(EmeiError::RpcError(err_str))
                }
            }
            Err(_) => Err(EmeiError::RpcTimeout),
        }
    }
}

/// Attempt to extract revert data bytes from an RPC error string.
///
/// Alloy error messages may contain hex-encoded revert data prefixed with "0x".
/// This function tries to find and decode it.
fn extract_revert_data(err_str: &str) -> Option<Vec<u8>> {
    // Look for a hex-encoded revert data pattern in the error
    // Common patterns: "execution reverted: 0x..." or "revert: 0x..."
    if let Some(idx) = err_str.find("0x") {
        let hex_part = &err_str[idx + 2..];
        // Take only hex characters
        let hex_chars: String = hex_part
            .chars()
            .take_while(|c| c.is_ascii_hexdigit())
            .collect();
        if hex_chars.len() >= 8 {
            // At least a 4-byte selector
            return hex::decode(&hex_chars).ok();
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_revert_data_with_selector() {
        let err = "execution reverted: 0x08c379a0000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000";
        let data = extract_revert_data(err);
        assert!(data.is_some());
        let data = data.unwrap();
        assert_eq!(&data[..4], &[0x08, 0xc3, 0x79, 0xa0]);
    }

    #[test]
    fn test_extract_revert_data_no_hex() {
        let err = "connection refused";
        assert!(extract_revert_data(err).is_none());
    }

    #[test]
    fn test_extract_revert_data_short_hex() {
        // Less than 4 bytes (8 hex chars) should return None
        let err = "error: 0x1234";
        assert!(extract_revert_data(err).is_none());
    }

    #[test]
    fn test_alloy_chain_client_new_invalid_url() {
        let key = B256::from([1u8; 32]);
        let result = AlloyChainClient::new("not a valid url", key);
        assert!(result.is_err());
    }

    #[test]
    fn test_alloy_chain_client_new_valid() {
        let key = B256::from_slice(
            &hex::decode("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")
                .unwrap(),
        );
        let result = AlloyChainClient::new("http://localhost:8545", key);
        assert!(result.is_ok());
    }
}
