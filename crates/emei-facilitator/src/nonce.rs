//! Nonce manager for serializing transaction submissions.
//!
//! Ensures that concurrent `send_hot` and `send_user` calls don't
//! produce nonce conflicts by tracking the next nonce per address
//! and serializing access with a mutex.

use std::collections::HashMap;

use alloy_primitives::Address;
use tokio::sync::Mutex;

/// Thread-safe nonce manager that tracks the next nonce for each address.
///
/// On first use for an address, queries the chain for the current nonce.
/// Subsequent calls increment locally without an RPC round-trip.
pub struct NonceManager {
    inner: Mutex<HashMap<Address, u64>>,
}

impl NonceManager {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
        }
    }

    /// Get the next nonce for an address. If this is the first call for
    /// this address, `chain_nonce` should be the current on-chain nonce.
    /// Subsequent calls auto-increment.
    pub async fn next_nonce(&self, address: Address, chain_nonce: Option<u64>) -> u64 {
        let mut map = self.inner.lock().await;
        let entry = map
            .entry(address)
            .or_insert_with(|| chain_nonce.unwrap_or(0));
        let nonce = *entry;
        *entry += 1;
        nonce
    }

    /// Called when a transaction is confirmed. Ensures the confirmed nonce
    /// is at least `nonce + 1` (handles out-of-order confirmations).
    pub async fn confirm(&self, address: Address, nonce: u64) {
        let mut map = self.inner.lock().await;
        let entry = map.entry(address).or_insert(nonce + 1);
        if *entry <= nonce {
            *entry = nonce + 1;
        }
    }

    /// Called when a transaction fails and the nonce should be reused.
    /// Only resets if the current next-nonce is exactly `nonce + 1`
    /// (i.e., no other tx has claimed a higher nonce since).
    pub async fn release(&self, address: Address, nonce: u64) {
        let mut map = self.inner.lock().await;
        if let Some(entry) = map.get_mut(&address) {
            if *entry == nonce + 1 {
                *entry = nonce;
            }
        }
    }

    /// Reset the nonce for an address (e.g., after detecting nonce-too-low).
    pub async fn reset(&self, address: Address, chain_nonce: u64) {
        let mut map = self.inner.lock().await;
        map.insert(address, chain_nonce);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[tokio::test]
    async fn test_sequential_nonces() {
        let mgr = NonceManager::new();
        let addr = address!("0000000000000000000000000000000000000001");

        assert_eq!(mgr.next_nonce(addr, Some(5)).await, 5);
        assert_eq!(mgr.next_nonce(addr, None).await, 6);
        assert_eq!(mgr.next_nonce(addr, None).await, 7);
    }

    #[tokio::test]
    async fn test_confirm_advances() {
        let mgr = NonceManager::new();
        let addr = address!("0000000000000000000000000000000000000001");

        mgr.next_nonce(addr, Some(0)).await; // claims 0
        mgr.confirm(addr, 0).await;
        // next should still be 1 (already incremented)
        assert_eq!(mgr.next_nonce(addr, None).await, 1);
    }

    #[tokio::test]
    async fn test_release_allows_reuse() {
        let mgr = NonceManager::new();
        let addr = address!("0000000000000000000000000000000000000001");

        let n = mgr.next_nonce(addr, Some(10)).await; // claims 10, next=11
        assert_eq!(n, 10);
        mgr.release(addr, 10).await; // release 10, next=10
        assert_eq!(mgr.next_nonce(addr, None).await, 10); // reuse 10
    }

    #[tokio::test]
    async fn test_release_no_op_if_higher_claimed() {
        let mgr = NonceManager::new();
        let addr = address!("0000000000000000000000000000000000000001");

        mgr.next_nonce(addr, Some(10)).await; // claims 10, next=11
        mgr.next_nonce(addr, None).await; // claims 11, next=12
        mgr.release(addr, 10).await; // can't release 10, 11 already claimed
        assert_eq!(mgr.next_nonce(addr, None).await, 12); // continues from 12
    }

    #[tokio::test]
    async fn test_reset() {
        let mgr = NonceManager::new();
        let addr = address!("0000000000000000000000000000000000000001");

        mgr.next_nonce(addr, Some(100)).await;
        mgr.next_nonce(addr, None).await;
        mgr.reset(addr, 50).await;
        assert_eq!(mgr.next_nonce(addr, None).await, 50);
    }

    #[tokio::test]
    async fn test_multiple_addresses() {
        let mgr = NonceManager::new();
        let a = address!("0000000000000000000000000000000000000001");
        let b = address!("0000000000000000000000000000000000000002");

        assert_eq!(mgr.next_nonce(a, Some(0)).await, 0);
        assert_eq!(mgr.next_nonce(b, Some(5)).await, 5);
        assert_eq!(mgr.next_nonce(a, None).await, 1);
        assert_eq!(mgr.next_nonce(b, None).await, 6);
    }
}
