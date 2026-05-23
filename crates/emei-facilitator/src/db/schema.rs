//! DDL constants for the EMEI SQLite database schema.

/// SQL statements to create the events table, indexes, and indexer state table.
/// Uses `IF NOT EXISTS` so it is safe to run on every startup.
pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    event_type TEXT NOT NULL,
    block_number INTEGER NOT NULL,
    tx_hash TEXT NOT NULL,
    log_index INTEGER NOT NULL,
    timestamp INTEGER NOT NULL,
    invoice_id INTEGER,
    payer TEXT,
    issuer TEXT,
    amount TEXT,
    params TEXT NOT NULL,
    UNIQUE(tx_hash, log_index)
);

CREATE INDEX IF NOT EXISTS idx_events_payer ON events(payer);
CREATE INDEX IF NOT EXISTS idx_events_issuer ON events(issuer);
CREATE INDEX IF NOT EXISTS idx_events_invoice_id ON events(invoice_id);
CREATE INDEX IF NOT EXISTS idx_events_block_number ON events(block_number);
CREATE INDEX IF NOT EXISTS idx_events_type_payer ON events(event_type, payer);

CREATE TABLE IF NOT EXISTS indexer_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pending_receipts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    receipt_hash BLOB NOT NULL,
    invoice_id INTEGER,
    created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS pending_txs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    tx_hash TEXT NOT NULL UNIQUE,
    sender TEXT NOT NULL,
    nonce INTEGER NOT NULL,
    submitted_at INTEGER NOT NULL,
    confirmed_at INTEGER,
    status TEXT NOT NULL DEFAULT 'pending'
);
"#;
