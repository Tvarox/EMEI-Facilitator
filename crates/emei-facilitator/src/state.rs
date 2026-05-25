use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::chain::ChainClient;
use crate::config::EmeiConfig;
use crate::db::StatementStore;
use crate::redis_client::RedisClient;

/// In-memory receipt queue (fast path — Redis is the durable queue).
pub struct ReceiptQueue {
    inner: Mutex<Vec<[u8; 32]>>,
}

impl ReceiptQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    pub async fn push(&self, receipt_hash: [u8; 32]) {
        self.inner.lock().await.push(receipt_hash);
    }

    pub async fn drain(&self) -> Vec<[u8; 32]> {
        let mut guard = self.inner.lock().await;
        std::mem::take(&mut *guard)
    }

    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    pub async fn extend(&self, receipts: Vec<[u8; 32]>) {
        self.inner.lock().await.extend(receipts);
    }
}

/// Shared application state.
pub struct AppState {
    pub chain: Arc<dyn ChainClient>,
    pub db: StatementStore,
    pub redis: RedisClient,
    pub receipt_queue: ReceiptQueue,
    pub config: EmeiConfig,
    pub cancel: CancellationToken,
    pub started_at: std::time::Instant,
}
