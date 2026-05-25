//! TX Queue database operations for the durable transaction job queue.

use sqlx::Row;

use crate::error::EmeiError;

use super::StatementStore;

/// A pending transaction job in the queue.
#[derive(Debug, Clone)]
pub struct TxJob {
    pub id: i64,
    pub to_address: String,
    pub calldata: Vec<u8>,
    pub priority: i16,
    pub source: String,
}

/// Result of a confirmed transaction.
#[derive(Debug, Clone)]
pub struct TxResult {
    pub job_id: i64,
    pub tx_hash: String,
    pub block_number: u64,
}

impl StatementStore {
    /// Enqueue a new transaction job. Returns the job ID.
    pub async fn enqueue_tx(
        &self,
        to_address: &str,
        calldata: &[u8],
        priority: i16,
        source: &str,
    ) -> Result<i64, EmeiError> {
        let now = now_ts() as i64;
        let row = sqlx::query(
            "INSERT INTO tx_queue (to_address, calldata, priority, source, created_at) VALUES ($1, $2, $3, $4, $5) RETURNING id"
        )
        .bind(to_address)
        .bind(calldata)
        .bind(priority)
        .bind(source)
        .bind(now)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("enqueue_tx failed: {e}")))?;

        Ok(row.get::<i64, _>("id"))
    }

    /// Claim the next pending job for a wallet. Uses SKIP LOCKED for lock-free concurrency.
    /// Returns None if no jobs are available.
    pub async fn claim_tx_job(&self, wallet_id: &str) -> Result<Option<TxJob>, EmeiError> {
        let now = now_ts() as i64;
        let row = sqlx::query(
            r#"UPDATE tx_queue SET status = 'assigned', wallet_id = $1, assigned_at = $2
               WHERE id = (
                   SELECT id FROM tx_queue
                   WHERE status = 'pending'
                   ORDER BY priority DESC, id ASC
                   LIMIT 1
                   FOR UPDATE SKIP LOCKED
               )
               RETURNING id, to_address, calldata, priority, source"#,
        )
        .bind(wallet_id)
        .bind(now)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("claim_tx_job failed: {e}")))?;

        Ok(row.map(|r| TxJob {
            id: r.get("id"),
            to_address: r.get("to_address"),
            calldata: r.get("calldata"),
            priority: r.get("priority"),
            source: r.get("source"),
        }))
    }

    /// Mark a job as submitted (tx sent to chain, awaiting receipt).
    pub async fn mark_tx_submitted(
        &self,
        job_id: i64,
        tx_hash: &str,
        nonce: u64,
    ) -> Result<(), EmeiError> {
        let now = now_ts() as i64;
        sqlx::query(
            "UPDATE tx_queue SET status = 'submitted', tx_hash = $1, nonce = $2, submitted_at = $3 WHERE id = $4"
        )
        .bind(tx_hash)
        .bind(nonce as i64)
        .bind(now)
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("mark_tx_submitted failed: {e}")))?;
        Ok(())
    }

    /// Mark a job as confirmed (receipt received, tx is on-chain).
    pub async fn mark_tx_confirmed(&self, job_id: i64, block_number: u64) -> Result<(), EmeiError> {
        let now = now_ts() as i64;
        sqlx::query(
            "UPDATE tx_queue SET status = 'confirmed', confirmed_at = $1, block_number = $2 WHERE id = $3"
        )
        .bind(now)
        .bind(block_number as i64)
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("mark_tx_confirmed failed: {e}")))?;
        Ok(())
    }

    /// Mark a job as failed. If retries < max_retries, reset to pending for retry.
    pub async fn mark_tx_failed(&self, job_id: i64, error: &str) -> Result<(), EmeiError> {
        // First increment retries and set error
        sqlx::query(
            "UPDATE tx_queue SET retries = retries + 1, error = $1, wallet_id = NULL, assigned_at = NULL WHERE id = $2"
        )
        .bind(error)
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("mark_tx_failed (1) failed: {e}")))?;

        // If retries < max_retries, reset to pending
        sqlx::query(
            "UPDATE tx_queue SET status = 'pending' WHERE id = $1 AND retries < max_retries",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("mark_tx_failed (2) failed: {e}")))?;

        // If retries >= max_retries, mark as permanently failed
        sqlx::query(
            "UPDATE tx_queue SET status = 'failed' WHERE id = $1 AND retries >= max_retries",
        )
        .bind(job_id)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("mark_tx_failed (3) failed: {e}")))?;

        Ok(())
    }

    /// Reclaim stuck jobs (assigned but not confirmed within timeout_secs).
    pub async fn reclaim_stuck_jobs(&self, timeout_secs: u64) -> Result<u64, EmeiError> {
        let cutoff = (now_ts() - timeout_secs) as i64;
        let result = sqlx::query(
            "UPDATE tx_queue SET status = 'pending', wallet_id = NULL, assigned_at = NULL WHERE status IN ('assigned', 'submitted') AND assigned_at < $1 AND retries < max_retries"
        )
        .bind(cutoff)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("reclaim_stuck_jobs failed: {e}")))?;

        Ok(result.rows_affected())
    }

    /// Get queue stats for the ops dashboard.
    pub async fn tx_queue_stats(&self) -> Result<Vec<(String, i64)>, EmeiError> {
        let rows = sqlx::query("SELECT status, COUNT(*) as cnt FROM tx_queue GROUP BY status")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("tx_queue_stats failed: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("status"), r.get::<i64, _>("cnt")))
            .collect())
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
