/// This module defines the `ChainClient` trait for interacting with the blockchain, and an implementation `AlloyChainClient` that uses the alloy-rs library to send transactions and make calls to the Mantle Sepolia testnet. It also includes error handling to decode revert reasons from failed transactions.
use alloy_network::{Ethereum, EthereumWallet, TransactionBuilder};
use alloy_primitives::{Address, Bytes, B256};
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_types_eth::TransactionRequest;
use alloy_signer_local::PrivateKeySigner;
use alloy_transport_http::reqwest;

use crate::error::{decode_revert, EmeiError};
use crate::redis_client::RedisClient;

#[async_trait::async_trait]
pub trait ChainClient: Send + Sync + 'static {
    /// Submit a transaction using the hot wallet signer with Redis-managed nonce.
    async fn send_hot(
        &self,
        to: Address,
        calldata: Bytes,
        redis: &RedisClient,
    ) -> Result<B256, EmeiError>;

    /// Submit a transaction using a user-provided signer (nonce auto-filled by provider).
    async fn send_user(
        &self,
        signer: PrivateKeySigner,
        to: Address,
        calldata: Bytes,
    ) -> Result<B256, EmeiError>;

    /// Execute a read-only call (no signing required).
    async fn call(&self, to: Address, calldata: Bytes) -> Result<Bytes, EmeiError>;

    /// Get the current on-chain nonce for the hot wallet address.
    async fn get_hot_nonce(&self) -> Result<u64, EmeiError>;
}

/// Production chain client using alloy-rs HTTP provider.
pub struct AlloyChainClient {
    provider: RootProvider<Ethereum>,
    hot_wallet: PrivateKeySigner,
    hot_address: Address,
}

impl AlloyChainClient {
    /// Create a new `AlloyChainClient` from an RPC URL and hot wallet private key.
    pub fn new(rpc_url: &str, hot_wallet_key: B256) -> Result<Self, EmeiError> {
        let url: reqwest::Url = rpc_url
            .parse()
            .map_err(|e| EmeiError::Internal(format!("invalid RPC URL: {e}")))?;

        let provider = ProviderBuilder::default().connect_http(url);

        let hot_wallet = PrivateKeySigner::from_bytes(&hot_wallet_key)
            .map_err(|e| EmeiError::Internal(format!("invalid hot wallet key: {e}")))?;

        let hot_address = hot_wallet.address();

        Ok(Self {
            provider,
            hot_wallet,
            hot_address,
        })
    }

    /// Send a transaction with an explicit nonce, signed by the given wallet.
    async fn send_with_nonce(
        &self,
        wallet: EthereumWallet,
        to: Address,
        calldata: Bytes,
        nonce: u64,
    ) -> Result<B256, EmeiError> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata)
            .with_nonce(nonce);

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_provider(&self.provider);

        let pending = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            provider.send_transaction(tx),
        )
        .await
        .map_err(|_| EmeiError::RpcTimeout)?
        .map_err(|e| Self::map_send_error(e.to_string()))?;

        Ok(*pending.tx_hash())
    }

    /// Send a transaction signed by the given wallet (nonce auto-filled by provider).
    async fn send_with_wallet(
        &self,
        wallet: EthereumWallet,
        to: Address,
        calldata: Bytes,
    ) -> Result<B256, EmeiError> {
        let tx = TransactionRequest::default()
            .with_to(to)
            .with_input(calldata);

        let provider = ProviderBuilder::new()
            .wallet(wallet)
            .connect_provider(&self.provider);

        let pending = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            provider.send_transaction(tx),
        )
        .await
        .map_err(|_| EmeiError::RpcTimeout)?
        .map_err(|e| Self::map_send_error(e.to_string()))?;

        Ok(*pending.tx_hash())
    }

    fn map_send_error(err_str: String) -> EmeiError {
        if err_str.contains("insufficient funds") {
            let detail = if let (Some(bal_idx), Some(cost_idx)) =
                (err_str.find("balance "), err_str.find("tx cost "))
            {
                let balance = err_str[bal_idx + 8..]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("?");
                let cost = err_str[cost_idx + 8..]
                    .split(|c: char| !c.is_ascii_digit())
                    .next()
                    .unwrap_or("?");
                format!(
                    "Insufficient MNT for gas. Balance: {} wei, cost: {} wei.",
                    balance, cost
                )
            } else {
                "Account does not have enough MNT for gas.".into()
            };
            return EmeiError::InsufficientFunds(detail);
        }
        if err_str.contains("nonce too low") {
            return EmeiError::Conflict(
                "Nonce too low — will re-sync from chain on next attempt.".into(),
            );
        }
        if let Some(data) = extract_revert_data(&err_str) {
            decode_revert(&data)
        } else {
            EmeiError::RpcError(err_str)
        }
    }
}

#[async_trait::async_trait]
impl ChainClient for AlloyChainClient {
    async fn send_hot(
        &self,
        to: Address,
        calldata: Bytes,
        redis: &RedisClient,
    ) -> Result<B256, EmeiError> {
        let wallet = EthereumWallet::from(self.hot_wallet.clone());
        let address_str = format!("0x{}", hex::encode(self.hot_address));

        // Get chain nonce for initialization (only used on first call / after reset)
        let chain_nonce = self.get_hot_nonce().await?;

        // Acquire next nonce atomically from Redis
        let nonce = redis.next_nonce(&address_str, chain_nonce).await?;

        match self.send_with_nonce(wallet, to, calldata, nonce).await {
            Ok(tx_hash) => Ok(tx_hash),
            Err(e) => {
                // On nonce-too-low, re-sync Redis from chain and retry once
                if e.to_string().contains("nonce too low")
                    || e.to_string().contains("Nonce too low")
                {
                    let fresh_nonce = self.get_hot_nonce().await?;
                    redis.reset_nonce(&address_str, fresh_nonce).await?;
                    tracing::warn!(
                        service = "chain",
                        old_nonce = nonce,
                        new_nonce = fresh_nonce,
                        "nonce re-synced from chain"
                    );
                    // Don't retry here — let the caller's next cycle handle it
                }
                // On other failures, release the nonce so it can be reused
                else {
                    let _ = redis.release_nonce(&address_str).await;
                }
                Err(e)
            }
        }
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
            tokio::time::timeout(std::time::Duration::from_secs(30), self.provider.call(tx)).await;

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

    async fn get_hot_nonce(&self) -> Result<u64, EmeiError> {
        let nonce = self
            .provider
            .get_transaction_count(self.hot_address)
            .await
            .map_err(|e| EmeiError::RpcError(format!("get_transaction_count: {e}")))?;
        Ok(nonce)
    }
}

/// Attempt to extract hex-encoded revert data from an RPC error message. Returns None if no hex data is found or if it's too short to be valid revert data.
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
