//! SQLite storage layer for indexed contract events.
//!
//! Provides the `StatementStore` for persisting and querying
//! on-chain events used by the statement endpoint.

pub mod queries;
pub mod schema;

use tokio_rusqlite::Connection;

use crate::error::EmeiError;

/// Represents an indexed contract event stored in SQLite.
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

/// Query parameters for the /statement endpoint.
#[derive(Debug)]
pub struct StatementQuery {
    pub payer: String,
    pub status: Option<String>,
    pub from: Option<u64>,
    pub to: Option<u64>,
    pub offset: u64,
    pub limit: u64,
}

/// SQLite-backed event store for indexed contract events.
pub struct StatementStore {
    conn: Connection,
}

impl StatementStore {
    /// Open (or create) the SQLite database at the given path.
    /// Applies WAL journal mode, sets synchronous to normal, and
    /// runs the schema DDL to ensure tables and indexes exist.
    pub async fn open(path: &str) -> Result<Self, EmeiError> {
        let conn = Connection::open(path)
            .await
            .map_err(|e| EmeiError::Database(format!("failed to open SQLite: {e}")))?;

        conn.call(|conn| {
            conn.pragma_update(None, "journal_mode", "wal")?;
            conn.pragma_update(None, "synchronous", "normal")?;
            conn.execute_batch(schema::SCHEMA_SQL)?;
            Ok(())
        })
        .await
        .map_err(|e| EmeiError::Database(format!("schema init failed: {e}")))?;

        Ok(Self { conn })
    }

    /// Insert a single indexed event into the database.
    /// Duplicate (tx_hash, log_index) pairs are silently ignored.
    pub async fn insert_event(&self, event: &IndexedEvent) -> Result<(), EmeiError> {
        let event = event.clone();
        self.conn
            .call(move |conn| {
                queries::insert_event(conn, &event)?;
                Ok(())
            })
            .await
            .map_err(|e| EmeiError::Database(format!("insert_event failed: {e}")))?;
        Ok(())
    }

    /// Query events matching the given statement parameters.
    /// Supports filtering by payer (required), status, date range,
    /// and pagination with ORDER BY block_number DESC.
    pub async fn query_statement(
        &self,
        query: &StatementQuery,
    ) -> Result<Vec<IndexedEvent>, EmeiError> {
        let payer = query.payer.clone();
        let status = query.status.clone();
        let from = query.from;
        let to = query.to;
        let offset = query.offset;
        let limit = query.limit;

        self.conn
            .call(move |conn| {
                let q = StatementQuery {
                    payer,
                    status,
                    from,
                    to,
                    offset,
                    limit,
                };
                let results = queries::query_statement(conn, &q)?;
                Ok(results)
            })
            .await
            .map_err(|e| EmeiError::Database(format!("query_statement failed: {e}")))
    }

    /// Get the last indexed block number, or None if no blocks have been indexed.
    pub async fn get_last_block(&self) -> Result<Option<u64>, EmeiError> {
        self.conn
            .call(|conn| {
                let block = queries::get_last_block(conn)?;
                Ok(block)
            })
            .await
            .map_err(|e| EmeiError::Database(format!("get_last_block failed: {e}")))
    }

    /// Persist the last indexed block number for restart recovery.
    pub async fn set_last_block(&self, block: u64) -> Result<(), EmeiError> {
        self.conn
            .call(move |conn| {
                queries::set_last_block(conn, block)?;
                Ok(())
            })
            .await
            .map_err(|e| EmeiError::Database(format!("set_last_block failed: {e}")))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    /// Helper to create a StatementStore backed by a temporary file.
    async fn test_store() -> (StatementStore, NamedTempFile) {
        let tmp = NamedTempFile::new().expect("failed to create temp file");
        let path = tmp.path().to_str().unwrap().to_string();
        let store = StatementStore::open(&path)
            .await
            .expect("failed to open store");
        (store, tmp)
    }

    fn sample_event(payer: &str, block: u64, event_type: &str) -> IndexedEvent {
        IndexedEvent {
            event_type: event_type.to_string(),
            block_number: block,
            tx_hash: format!("0x{:064x}", block),
            log_index: 0,
            timestamp: 1700000000 + block,
            invoice_id: Some(block),
            payer: Some(payer.to_string()),
            issuer: Some("0x1111111111111111111111111111111111111111".to_string()),
            amount: Some("1000000".to_string()),
            params: "{}".to_string(),
        }
    }

    #[tokio::test]
    async fn test_schema_creation() {
        let (store, _tmp) = test_store().await;
        // Verify we can insert and query without errors (schema exists)
        let result = store.get_last_block().await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), None);
    }

    #[tokio::test]
    async fn test_insert_and_query_round_trip() {
        let (store, _tmp) = test_store().await;

        let event = sample_event("0xaaaa", 100, "InvoiceCreated");
        store.insert_event(&event).await.unwrap();

        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 0,
            limit: 100,
        };

        let results = store.query_statement(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, "InvoiceCreated");
        assert_eq!(results[0].block_number, 100);
        assert_eq!(results[0].tx_hash, format!("0x{:064x}", 100));
        assert_eq!(results[0].log_index, 0);
        assert_eq!(results[0].timestamp, 1700000000 + 100);
        assert_eq!(results[0].invoice_id, Some(100));
        assert_eq!(results[0].payer.as_deref(), Some("0xaaaa"));
        assert_eq!(results[0].amount.as_deref(), Some("1000000"));
        assert_eq!(results[0].params, "{}");
    }

    #[tokio::test]
    async fn test_filter_by_payer() {
        let (store, _tmp) = test_store().await;

        store
            .insert_event(&sample_event("0xaaaa", 1, "InvoiceCreated"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xbbbb", 2, "InvoiceCreated"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xaaaa", 3, "InvoicePaid"))
            .await
            .unwrap();

        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 0,
            limit: 100,
        };

        let results = store.query_statement(&query).await.unwrap();
        assert_eq!(results.len(), 2);
        // Should be ordered by block_number DESC
        assert_eq!(results[0].block_number, 3);
        assert_eq!(results[1].block_number, 1);

        // Query for the other payer
        let query_b = StatementQuery {
            payer: "0xbbbb".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 0,
            limit: 100,
        };
        let results_b = store.query_statement(&query_b).await.unwrap();
        assert_eq!(results_b.len(), 1);
        assert_eq!(results_b[0].block_number, 2);
    }

    #[tokio::test]
    async fn test_pagination() {
        let (store, _tmp) = test_store().await;

        // Insert 5 events for the same payer
        for i in 1..=5 {
            store
                .insert_event(&sample_event("0xaaaa", i, "InvoiceCreated"))
                .await
                .unwrap();
        }

        // Page 1: limit 2, offset 0
        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 0,
            limit: 2,
        };
        let page1 = store.query_statement(&query).await.unwrap();
        assert_eq!(page1.len(), 2);
        assert_eq!(page1[0].block_number, 5); // DESC order
        assert_eq!(page1[1].block_number, 4);

        // Page 2: limit 2, offset 2
        let query2 = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 2,
            limit: 2,
        };
        let page2 = store.query_statement(&query2).await.unwrap();
        assert_eq!(page2.len(), 2);
        assert_eq!(page2[0].block_number, 3);
        assert_eq!(page2[1].block_number, 2);

        // Page 3: limit 2, offset 4
        let query3 = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 4,
            limit: 2,
        };
        let page3 = store.query_statement(&query3).await.unwrap();
        assert_eq!(page3.len(), 1);
        assert_eq!(page3[0].block_number, 1);
    }

    #[tokio::test]
    async fn test_filter_by_status() {
        let (store, _tmp) = test_store().await;

        store
            .insert_event(&sample_event("0xaaaa", 1, "InvoiceCreated"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xaaaa", 2, "InvoicePaid"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xaaaa", 3, "InvoiceCreated"))
            .await
            .unwrap();

        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: Some("InvoicePaid".to_string()),
            from: None,
            to: None,
            offset: 0,
            limit: 100,
        };
        let results = store.query_statement(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].event_type, "InvoicePaid");
    }

    #[tokio::test]
    async fn test_filter_by_date_range() {
        let (store, _tmp) = test_store().await;

        // timestamps: 1700000001, 1700000002, 1700000003
        store
            .insert_event(&sample_event("0xaaaa", 1, "InvoiceCreated"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xaaaa", 2, "InvoiceCreated"))
            .await
            .unwrap();
        store
            .insert_event(&sample_event("0xaaaa", 3, "InvoiceCreated"))
            .await
            .unwrap();

        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: Some(1700000002),
            to: Some(1700000002),
            offset: 0,
            limit: 100,
        };
        let results = store.query_statement(&query).await.unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].block_number, 2);
    }

    #[tokio::test]
    async fn test_last_block_round_trip() {
        let (store, _tmp) = test_store().await;

        // Initially None
        assert_eq!(store.get_last_block().await.unwrap(), None);

        // Set and get
        store.set_last_block(42).await.unwrap();
        assert_eq!(store.get_last_block().await.unwrap(), Some(42));

        // Update
        store.set_last_block(100).await.unwrap();
        assert_eq!(store.get_last_block().await.unwrap(), Some(100));
    }

    #[tokio::test]
    async fn test_duplicate_insert_ignored() {
        let (store, _tmp) = test_store().await;

        let event = sample_event("0xaaaa", 1, "InvoiceCreated");
        store.insert_event(&event).await.unwrap();
        // Insert same event again (same tx_hash + log_index) — should not error
        store.insert_event(&event).await.unwrap();

        let query = StatementQuery {
            payer: "0xaaaa".to_string(),
            status: None,
            from: None,
            to: None,
            offset: 0,
            limit: 100,
        };
        let results = store.query_statement(&query).await.unwrap();
        assert_eq!(results.len(), 1);
    }
}
