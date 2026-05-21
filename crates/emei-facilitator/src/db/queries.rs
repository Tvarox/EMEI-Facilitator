//! Query functions for the EMEI SQLite database.
//!
//! These are helper functions called within `tokio_rusqlite::Connection::call`
//! closures. They operate on a synchronous `rusqlite::Connection`.

use rusqlite::{Connection, OptionalExtension, params};

use super::{IndexedEvent, StatementQuery};

/// Insert a single event into the events table.
/// Uses INSERT OR IGNORE to handle duplicate (tx_hash, log_index) gracefully.
pub fn insert_event(conn: &Connection, event: &IndexedEvent) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO events (event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            event.event_type,
            event.block_number as i64,
            event.tx_hash,
            event.log_index as i64,
            event.timestamp as i64,
            event.invoice_id.map(|id| id as i64),
            event.payer,
            event.issuer,
            event.amount,
            event.params,
        ],
    )?;
    Ok(())
}

/// Query events matching the given statement query parameters.
/// Builds dynamic SQL with optional WHERE clauses for status (event_type),
/// date range, and pagination. Results are ordered by block_number DESC.
pub fn query_statement(
    conn: &Connection,
    query: &StatementQuery,
) -> rusqlite::Result<Vec<IndexedEvent>> {
    let mut sql = String::from(
        "SELECT event_type, block_number, tx_hash, log_index, timestamp, invoice_id, payer, issuer, amount, params FROM events WHERE payer = ?1",
    );
    let mut param_index = 2u32;

    // We'll collect boxed params for dynamic binding
    let mut dynamic_params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    dynamic_params.push(Box::new(query.payer.clone()));

    if let Some(ref status) = query.status {
        sql.push_str(&format!(" AND event_type = ?{param_index}"));
        dynamic_params.push(Box::new(status.clone()));
        param_index += 1;
    }

    if let Some(from) = query.from {
        sql.push_str(&format!(" AND timestamp >= ?{param_index}"));
        dynamic_params.push(Box::new(from as i64));
        param_index += 1;
    }

    if let Some(to) = query.to {
        sql.push_str(&format!(" AND timestamp <= ?{param_index}"));
        dynamic_params.push(Box::new(to as i64));
        param_index += 1;
    }

    sql.push_str(" ORDER BY block_number DESC");
    sql.push_str(&format!(" LIMIT ?{param_index}"));
    dynamic_params.push(Box::new(query.limit as i64));
    param_index += 1;

    sql.push_str(&format!(" OFFSET ?{param_index}"));
    dynamic_params.push(Box::new(query.offset as i64));

    let param_refs: Vec<&dyn rusqlite::types::ToSql> =
        dynamic_params.iter().map(|p| p.as_ref()).collect();

    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(param_refs.as_slice(), |row| {
        Ok(IndexedEvent {
            event_type: row.get(0)?,
            block_number: row.get::<_, i64>(1)? as u64,
            tx_hash: row.get(2)?,
            log_index: row.get::<_, i64>(3)? as u32,
            timestamp: row.get::<_, i64>(4)? as u64,
            invoice_id: row.get::<_, Option<i64>>(5)?.map(|v| v as u64),
            payer: row.get(6)?,
            issuer: row.get(7)?,
            amount: row.get(8)?,
            params: row.get(9)?,
        })
    })?;

    let mut events = Vec::new();
    for row in rows {
        events.push(row?);
    }
    Ok(events)
}

/// Get the last indexed block number from the indexer_state table.
pub fn get_last_block(conn: &Connection) -> rusqlite::Result<Option<u64>> {
    let result: Option<String> = conn
        .query_row(
            "SELECT value FROM indexer_state WHERE key = 'last_block_number'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    Ok(result.and_then(|v| v.parse::<u64>().ok()))
}

/// Set the last indexed block number in the indexer_state table.
pub fn set_last_block(conn: &Connection, block: u64) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO indexer_state (key, value) VALUES ('last_block_number', ?1)",
        params![block.to_string()],
    )?;
    Ok(())
}
