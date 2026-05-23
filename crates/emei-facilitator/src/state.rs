use std::sync::Arc;

use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::chain::ChainClient;
use crate::config::EmeiConfig;
use crate::db::StatementStore;
use crate::nonce::NonceManager;

/// In-memory queue for pending settlement receipts awaiting Merkle batching.
/// Note: In production, receipts are ALSO persisted to DB via `insert_pending_receipt`.
/// This in-memory queue is used as a fast path; the DB is the durable backup.
pub struct ReceiptQueue {
    inner: Mutex<Vec<[u8; 32]>>,
}

impl ReceiptQueue {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Vec::new()),
        }
    }

    /// Add a receipt hash to the queue.
    pub async fn push(&self, receipt_hash: [u8; 32]) {
        self.inner.lock().await.push(receipt_hash);
    }

    /// Drain all pending receipts from the queue atomically.
    pub async fn drain(&self) -> Vec<[u8; 32]> {
        let mut guard = self.inner.lock().await;
        std::mem::take(&mut *guard)
    }

    /// Return the number of pending receipts.
    pub async fn len(&self) -> usize {
        self.inner.lock().await.len()
    }

    /// Re-insert receipts (used on batch failure).
    pub async fn extend(&self, receipts: Vec<[u8; 32]>) {
        self.inner.lock().await.extend(receipts);
    }
}

/// Shared application state passed to all route handlers and background services.
pub struct AppState {
    /// Chain client for contract interactions
    pub chain: Arc<dyn ChainClient>,
    /// SQLite event store for statement queries
    pub db: StatementStore,
    /// In-memory receipt queue for batching (fast path)
    pub receipt_queue: ReceiptQueue,
    /// Nonce manager for serializing transaction submissions
    pub nonce_manager: NonceManager,
    /// Loaded configuration
    pub config: EmeiConfig,
    /// Cancellation token for graceful shutdown
    pub cancel: CancellationToken,
    /// Server start time for uptime reporting
    pub started_at: std::time::Instant,
}
