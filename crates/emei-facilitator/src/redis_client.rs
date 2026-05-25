//! Redis client for queues, caching, and nonce management.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

use crate::error::EmeiError;

/// Redis-backed queue, cache, and nonce manager.
#[derive(Clone)]
pub struct RedisClient {
    conn: ConnectionManager,
}

// Queue keys
const WEBHOOK_QUEUE: &str = "emei:queue:webhook";
const RECEIPT_QUEUE: &str = "emei:queue:receipts";

// Nonce key prefix
const NONCE_PREFIX: &str = "emei:nonce:";

// Cache keys
#[allow(dead_code)]
const STATS_CACHE: &str = "emei:cache:stats";

impl RedisClient {
    /// Connect to Redis.
    pub async fn new(redis_url: &str) -> Result<Self, EmeiError> {
        let client = redis::Client::open(redis_url)
            .map_err(|e| EmeiError::Internal(format!("redis client error: {e}")))?;
        let conn = ConnectionManager::new(client)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis connection failed: {e}")))?;
        Ok(Self { conn })
    }

    /// Push a raw webhook payload to the processing queue.
    pub async fn push_webhook(&self, payload: &str) -> Result<(), EmeiError> {
        let mut conn = self.conn.clone();
        conn.lpush::<_, _, ()>(WEBHOOK_QUEUE, payload)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis lpush failed: {e}")))?;
        Ok(())
    }

    /// Pop a webhook payload from the queue (blocking, 5s timeout).
    pub async fn pop_webhook(&self) -> Result<Option<String>, EmeiError> {
        let mut conn = self.conn.clone();
        let result: Option<(String, String)> = conn
            .brpop(WEBHOOK_QUEUE, 5.0)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis brpop failed: {e}")))?;
        Ok(result.map(|(_, val)| val))
    }

    /// Push a receipt hash to the persistent queue.
    pub async fn push_receipt(&self, hash: &[u8; 32]) -> Result<(), EmeiError> {
        let mut conn = self.conn.clone();
        conn.lpush::<_, _, ()>(RECEIPT_QUEUE, hash.as_slice())
            .await
            .map_err(|e| EmeiError::Internal(format!("redis receipt push failed: {e}")))?;
        Ok(())
    }

    /// Pop up to `count` receipts from the queue.
    pub async fn pop_receipts(&self, count: usize) -> Result<Vec<[u8; 32]>, EmeiError> {
        let mut conn = self.conn.clone();
        let mut results = Vec::new();
        for _ in 0..count {
            let val: Option<Vec<u8>> = conn
                .rpop(RECEIPT_QUEUE, None)
                .await
                .map_err(|e| EmeiError::Internal(format!("redis receipt pop failed: {e}")))?;
            match val {
                Some(bytes) if bytes.len() == 32 => {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    results.push(arr);
                }
                _ => break,
            }
        }
        Ok(results)
    }

    /// Get receipt queue length.
    pub async fn receipt_queue_len(&self) -> Result<usize, EmeiError> {
        let mut conn = self.conn.clone();
        let len: usize = conn
            .llen(RECEIPT_QUEUE)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis llen failed: {e}")))?;
        Ok(len)
    }

    /// Atomically get-and-increment the nonce for an address using INCR.
    /// On first call, initializes from `chain_nonce` via SETNX, then INCRs.
    /// Returns the nonce to use for the current transaction.
    pub async fn next_nonce(&self, address: &str, chain_nonce: u64) -> Result<u64, EmeiError> {
        let key = format!("{}{}", NONCE_PREFIX, address.to_lowercase());
        let mut conn = self.conn.clone();

        // SETNX: only sets if key doesn't exist (first boot or after reset)
        // We set to chain_nonce - 1 because INCR will bump it to chain_nonce on first use
        let was_set: bool = conn
            .set_nx(&key, chain_nonce.saturating_sub(1))
            .await
            .map_err(|e| EmeiError::Internal(format!("redis setnx nonce: {e}")))?;

        if was_set {
            // First time — INCR returns chain_nonce (since we set chain_nonce - 1)
            let val: u64 = conn
                .incr(&key, 1u64)
                .await
                .map_err(|e| EmeiError::Internal(format!("redis incr nonce: {e}")))?;
            return Ok(val);
        }

        // Key exists — atomic increment
        let val: u64 = conn
            .incr(&key, 1u64)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis incr nonce: {e}")))?;
        Ok(val)
    }

    /// Reset nonce for an address (e.g., after detecting nonce-too-low from chain).
    pub async fn reset_nonce(&self, address: &str, value: u64) -> Result<(), EmeiError> {
        let key = format!("{}{}", NONCE_PREFIX, address.to_lowercase());
        let mut conn = self.conn.clone();
        conn.set::<_, _, ()>(&key, value)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis reset nonce: {e}")))?;
        Ok(())
    }

    /// Decrement nonce (release) when a tx fails and nonce should be reused.
    pub async fn release_nonce(&self, address: &str) -> Result<(), EmeiError> {
        let key = format!("{}{}", NONCE_PREFIX, address.to_lowercase());
        let mut conn = self.conn.clone();
        let _: u64 = conn
            .decr(&key, 1u64)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis decr nonce: {e}")))?;
        Ok(())
    }

    /// Cache stats JSON with a TTL.
    pub async fn set_cache(&self, key: &str, value: &str, ttl_secs: u64) -> Result<(), EmeiError> {
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(key, value, ttl_secs)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis set_ex failed: {e}")))?;
        Ok(())
    }

    /// Get cached value.
    pub async fn get_cache(&self, key: &str) -> Result<Option<String>, EmeiError> {
        let mut conn = self.conn.clone();
        let val: Option<String> = conn
            .get(key)
            .await
            .map_err(|e| EmeiError::Internal(format!("redis get cache failed: {e}")))?;
        Ok(val)
    }

    /// Check if Redis is reachable.
    pub async fn ping(&self) -> bool {
        let mut conn = self.conn.clone();
        redis::cmd("PING")
            .query_async::<String>(&mut conn)
            .await
            .is_ok()
    }
}
