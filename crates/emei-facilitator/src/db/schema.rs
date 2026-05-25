//! PostgreSQL schema DDL for the EMEI database.

pub const SCHEMA_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS events (
    id BIGSERIAL PRIMARY KEY,
    event_type TEXT NOT NULL,
    block_number BIGINT NOT NULL DEFAULT 0,
    tx_hash TEXT NOT NULL,
    log_index INTEGER NOT NULL DEFAULT 0,
    timestamp BIGINT NOT NULL,
    invoice_id BIGINT,
    payer TEXT,
    issuer TEXT,
    amount TEXT,
    params TEXT NOT NULL DEFAULT '{}',
    status TEXT NOT NULL DEFAULT 'confirmed',
    UNIQUE(tx_hash, log_index)
);

-- Add status column if table was created before this migration
ALTER TABLE events ADD COLUMN IF NOT EXISTS status TEXT NOT NULL DEFAULT 'confirmed';

CREATE INDEX IF NOT EXISTS idx_events_payer ON events(payer);
CREATE INDEX IF NOT EXISTS idx_events_issuer ON events(issuer);
CREATE INDEX IF NOT EXISTS idx_events_invoice_id ON events(invoice_id);
CREATE INDEX IF NOT EXISTS idx_events_block_number ON events(block_number DESC);
CREATE INDEX IF NOT EXISTS idx_events_type ON events(event_type);
CREATE INDEX IF NOT EXISTS idx_events_status ON events(status);

CREATE TABLE IF NOT EXISTS indexer_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS pending_receipts (
    id BIGSERIAL PRIMARY KEY,
    receipt_hash BYTEA NOT NULL,
    invoice_id BIGINT,
    created_at BIGINT NOT NULL
);

CREATE TABLE IF NOT EXISTS pending_txs (
    id BIGSERIAL PRIMARY KEY,
    tx_hash TEXT NOT NULL UNIQUE,
    sender TEXT NOT NULL,
    nonce BIGINT NOT NULL,
    submitted_at BIGINT NOT NULL,
    confirmed_at BIGINT,
    status TEXT NOT NULL DEFAULT 'pending'
);
"#;
