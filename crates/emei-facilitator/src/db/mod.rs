//! PostgreSQL storage layer for indexed contract events.

pub mod schema;

use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};

use crate::error::EmeiError;

#[derive(Debug, Clone, serde::Serialize)]
pub struct IndexedEvent {
    pub event_type: String,
    pub block_number: u64,
    pub tx_hash: String,
    pub log_index: u32,
    pub timestamp: u64,
    pub invoice_id: Option<u64>,
    pub payer: Option<String>,
    pub issuer: Option<String>,
    pub amount: Option<String>,
    pub params: String,
}

#[derive(Debug)]
pub struct StatementQuery {
    pub payer: String,
    pub status: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub offset: u64,
    pub limit: u64,
}

pub struct StatementStore {
    pool: PgPool,
}

impl StatementStore {
    /// Connect to PostgreSQL and run schema migrations.
    pub async fn open(database_url: &str) -> Result<Self, EmeiError> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await
            .map_err(|e| EmeiError::Database(format!("failed to connect to Postgres: {e}")))?;

        // Run schema
        sqlx::raw_sql(schema::SCHEMA_SQL)
            .execute(&pool)
            .await
            .map_err(|e| EmeiError::Database(format!("schema migration failed: {e}")))?;

        Ok(Self { pool })
    }

    /// Insert an event with idempotency in mind
    pub async fn insert_event(&self, event: &IndexedEvent) -> Result<(), EmeiError> {
        sqlx::query(
            r#"INSERT INTO events (event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params, status)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending')
               ON CONFLICT (tx_hash, log_index) DO NOTHING"#,
        )
        .bind(&event.event_type)
        .bind(event.block_number as i64)
        .bind(&event.tx_hash)
        .bind(event.log_index as i32)
        .bind(event.timestamp as i64)
        .bind(event.invoice_id.map(|id| id as i64))
        .bind(&event.payer)
        .bind(&event.issuer)
        .bind(&event.amount)
        .bind(&event.params)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("insert_event failed: {e}")))?;

        Ok(())
    }

    /// Upsert an event as confirmed, allowing updates to certain fields while preserving others.
    pub async fn upsert_confirmed_event(&self, event: &IndexedEvent) -> Result<(), EmeiError> {
        sqlx::query(
            r#"INSERT INTO events (event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params, status)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'confirmed')
               ON CONFLICT (tx_hash, log_index) DO UPDATE SET
                 event_type = EXCLUDED.event_type,
                 block_number = EXCLUDED.block_number,
                 timestamp = EXCLUDED.timestamp,
                 invoice_id = COALESCE(EXCLUDED.invoice_id, events.invoice_id),
                 payer = COALESCE(EXCLUDED.payer, events.payer),
                 issuer = COALESCE(EXCLUDED.issuer, events.issuer),
                 amount = COALESCE(EXCLUDED.amount, events.amount),
                 params = EXCLUDED.params,
                 status = 'confirmed'"#,
        )
        .bind(&event.event_type)
        .bind(event.block_number as i64)
        .bind(&event.tx_hash)
        .bind(event.log_index as i32)
        .bind(event.timestamp as i64)
        .bind(event.invoice_id.map(|id| id as i64))
        .bind(&event.payer)
        .bind(&event.issuer)
        .bind(&event.amount)
        .bind(&event.params)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("upsert_confirmed_event failed: {e}")))?;

        Ok(())
    }

    /// Query events for a given payer with pagination.
    pub async fn query_statement(
        &self,
        query: &StatementQuery,
    ) -> Result<Vec<IndexedEvent>, EmeiError> {
        let rows = sqlx::query(
            r#"SELECT event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params
               FROM events WHERE payer = $1
               ORDER BY block_number DESC
               LIMIT $2 OFFSET $3"#,
        )
        .bind(&query.payer)
        .bind(query.limit as i64)
        .bind(query.offset as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("query_statement failed: {e}")))?;

        Ok(rows.iter().map(row_to_event).collect())
    }

    /// Get the last indexed block number from the database, or None if not set.
    pub async fn get_last_block(&self) -> Result<Option<u64>, EmeiError> {
        let row = sqlx::query("SELECT value FROM indexer_state WHERE key = 'last_block_number'")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("get_last_block failed: {e}")))?;

        Ok(row.and_then(|r| r.get::<String, _>("value").parse::<u64>().ok()))
    }

    pub async fn set_last_block(&self, block: u64) -> Result<(), EmeiError> {
        sqlx::query(
            "INSERT INTO indexer_state (key, value) VALUES ('last_block_number', $1) ON CONFLICT (key) DO UPDATE SET value = $1",
        )
        .bind(block.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("set_last_block failed: {e}")))?;
        Ok(())
    }

    // ─── Persistent receipt queue ─────────────────────────────────────────────

    pub async fn insert_pending_receipt(
        &self,
        receipt_hash: &[u8; 32],
        invoice_id: Option<u64>,
    ) -> Result<(), EmeiError> {
        let now = now_ts() as i64;
        sqlx::query("INSERT INTO pending_receipts (receipt_hash, invoice_id, created_at) VALUES ($1, $2, $3)")
            .bind(receipt_hash.as_slice())
            .bind(invoice_id.map(|id| id as i64))
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("insert_pending_receipt failed: {e}")))?;
        Ok(())
    }

    pub async fn drain_pending_receipts(
        &self,
        max_count: usize,
    ) -> Result<Vec<[u8; 32]>, EmeiError> {
        let rows = sqlx::query(
            "DELETE FROM pending_receipts WHERE id IN (SELECT id FROM pending_receipts ORDER BY id ASC LIMIT $1) RETURNING receipt_hash",
        )
        .bind(max_count as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("drain_pending_receipts failed: {e}")))?;

        let hashes: Vec<[u8; 32]> = rows
            .iter()
            .filter_map(|row| {
                let bytes: Vec<u8> = row.get("receipt_hash");
                if bytes.len() == 32 {
                    let mut arr = [0u8; 32];
                    arr.copy_from_slice(&bytes);
                    Some(arr)
                } else {
                    None
                }
            })
            .collect();

        Ok(hashes)
    }

    pub async fn count_pending_receipts(&self) -> Result<usize, EmeiError> {
        let row = sqlx::query("SELECT COUNT(*) as cnt FROM pending_receipts")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("count_pending_receipts failed: {e}")))?;
        Ok(row.get::<i64, _>("cnt") as usize)
    }

    // ─── Pending transaction tracking ─────────────────────────────────────────

    pub async fn insert_pending_tx(
        &self,
        tx_hash: &str,
        sender: &str,
        nonce: u64,
    ) -> Result<(), EmeiError> {
        let now = now_ts() as i64;
        sqlx::query(
            "INSERT INTO pending_txs (tx_hash, sender, nonce, submitted_at, status) VALUES ($1, $2, $3, $4, 'pending') ON CONFLICT (tx_hash) DO NOTHING",
        )
        .bind(tx_hash)
        .bind(sender)
        .bind(nonce as i64)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("insert_pending_tx failed: {e}")))?;
        Ok(())
    }

    pub async fn confirm_pending_tx(&self, tx_hash: &str) -> Result<(), EmeiError> {
        let now = now_ts() as i64;
        sqlx::query(
            "UPDATE pending_txs SET status = 'confirmed', confirmed_at = $1 WHERE tx_hash = $2",
        )
        .bind(now)
        .bind(tx_hash)
        .execute(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("confirm_pending_tx failed: {e}")))?;
        Ok(())
    }

    pub async fn get_stale_pending_txs(
        &self,
        age_secs: u64,
    ) -> Result<Vec<(String, String, u64)>, EmeiError> {
        let cutoff = (now_ts() - age_secs) as i64;
        let rows = sqlx::query("SELECT tx_hash, sender, nonce FROM pending_txs WHERE status = 'pending' AND submitted_at < $1")
            .bind(cutoff)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("get_stale_pending_txs failed: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| {
                (
                    r.get::<String, _>("tx_hash"),
                    r.get::<String, _>("sender"),
                    r.get::<i64, _>("nonce") as u64,
                )
            })
            .collect())
    }

    // ─── Public dashboard queries ────────────────────────────────────────────

    pub async fn count_events_by_type(&self) -> Result<Vec<(String, i64)>, EmeiError> {
        let rows =
            sqlx::query("SELECT event_type, COUNT(*) as cnt FROM events GROUP BY event_type")
                .fetch_all(&self.pool)
                .await
                .map_err(|e| EmeiError::Database(format!("count_events_by_type failed: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("event_type"), r.get::<i64, _>("cnt")))
            .collect())
    }

    pub async fn sum_amount_for_type(&self, event_type: &str) -> Result<u128, EmeiError> {
        let row = sqlx::query(
            "SELECT COALESCE(SUM(CAST(amount AS DOUBLE PRECISION)), 0) as total FROM events WHERE event_type = $1 AND amount IS NOT NULL",
        )
        .bind(event_type)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("sum_amount_for_type failed: {e}")))?;

        let total: f64 = row.get("total");
        Ok(total as u128)
    }

    pub async fn recent_events(
        &self,
        limit: i64,
        before_block: Option<i64>,
    ) -> Result<Vec<IndexedEvent>, EmeiError> {
        let rows = match before_block {
            Some(block) => {
                sqlx::query(
                    "SELECT event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params FROM events WHERE block_number < $1 ORDER BY block_number DESC, log_index DESC LIMIT $2",
                )
                .bind(block)
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
            None => {
                sqlx::query(
                    "SELECT event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params FROM events ORDER BY block_number DESC, log_index DESC LIMIT $1",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await
            }
        }
        .map_err(|e| EmeiError::Database(format!("recent_events failed: {e}")))?;

        Ok(rows.iter().map(row_to_event).collect())
    }

    pub async fn latest_receipt_event(&self) -> Result<Option<(String, i64)>, EmeiError> {
        let row = sqlx::query(
            "SELECT params, timestamp FROM events WHERE event_type = 'MerkleRootPosted' ORDER BY block_number DESC LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("latest_receipt_event failed: {e}")))?;

        Ok(row.map(|r| (r.get::<String, _>("params"), r.get::<i64, _>("timestamp"))))
    }

    pub async fn count_events_for_issuer(
        &self,
        issuer: &str,
    ) -> Result<Vec<(String, i64)>, EmeiError> {
        let rows = sqlx::query(
            "SELECT event_type, COUNT(*) as cnt FROM events WHERE issuer = $1 GROUP BY event_type",
        )
        .bind(issuer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("count_events_for_issuer failed: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("event_type"), r.get::<i64, _>("cnt")))
            .collect())
    }

    pub async fn count_events_for_payer(
        &self,
        payer: &str,
    ) -> Result<Vec<(String, i64)>, EmeiError> {
        let rows = sqlx::query(
            "SELECT event_type, COUNT(*) as cnt FROM events WHERE payer = $1 GROUP BY event_type",
        )
        .bind(payer)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| EmeiError::Database(format!("count_events_for_payer failed: {e}")))?;

        Ok(rows
            .iter()
            .map(|r| (r.get::<String, _>("event_type"), r.get::<i64, _>("cnt")))
            .collect())
    }

    pub async fn latest_block(&self) -> Result<Option<i64>, EmeiError> {
        let row = sqlx::query("SELECT MAX(block_number) as max_block FROM events")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| EmeiError::Database(format!("latest_block failed: {e}")))?;

        Ok(row.get::<Option<i64>, _>("max_block"))
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn row_to_event(row: &sqlx::postgres::PgRow) -> IndexedEvent {
    IndexedEvent {
        event_type: row.get("event_type"),
        block_number: row.get::<i64, _>("block_number") as u64,
        tx_hash: row.get("tx_hash"),
        log_index: row.get::<i32, _>("log_index") as u32,
        timestamp: row.get::<i64, _>("timestamp") as u64,
        invoice_id: row.get::<Option<i64>, _>("invoice_id").map(|v| v as u64),
        payer: row.get("payer"),
        issuer: row.get("issuer"),
        amount: row.get("amount"),
        params: row.get("params"),
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
