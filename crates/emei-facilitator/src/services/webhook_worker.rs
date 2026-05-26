// This module implements the webhook worker that listens for Alchemy webhook payloads about
// invoice events, parses them, and updates the database accordingly. It also triggers
// reputation feedback when payments are confirmed.
use std::sync::Arc;
use std::time::Duration;

use alloy_primitives::U256;
#[allow(unused_imports)]
use alloy_sol_types::SolCall;
use serde::Deserialize;
use tokio_util::sync::CancellationToken;

use crate::db::IndexedEvent;
use crate::state::AppState;

#[derive(Deserialize)]
struct AlchemyPayload {
    event: Option<AlchemyEvent>,
}

#[derive(Deserialize)]
struct AlchemyEvent {
    activity: Option<Vec<AlchemyActivity>>,
}

#[derive(Deserialize)]
struct AlchemyActivity {
    hash: Option<String>,
    #[allow(dead_code)]
    #[serde(rename = "blockNum")]
    block_num: Option<String>,
    log: Option<AlchemyLog>,
}

#[derive(Deserialize)]
struct AlchemyLog {
    topics: Option<Vec<String>>,
    data: Option<String>,
    #[serde(rename = "logIndex")]
    log_index: Option<String>,
    #[serde(rename = "blockNumber")]
    block_number: Option<String>,
    #[serde(rename = "transactionHash")]
    transaction_hash: Option<String>,
}

/// Background worker that pops webhook payloads from Redis and processes them.
pub async fn webhook_worker(state: Arc<AppState>, cancel: CancellationToken) {
    tracing::info!("webhook_worker: started (polling mode)");

    // Verify Redis connectivity from this task
    match state.redis.webhook_queue_len().await {
        Ok(len) => tracing::info!(queue_len = len, "webhook_worker: redis check OK"),
        Err(e) => {
            tracing::error!(error = %e, "webhook_worker: redis check FAILED — worker will not process webhooks");
            return;
        }
    }

    loop {
        if cancel.is_cancelled() {
            tracing::info!("webhook_worker: shutting down");
            break;
        }

        match state.redis.pop_webhook().await {
            Ok(Some(payload)) => {
                tracing::info!(len = payload.len(), "webhook_worker: popped payload");
                if let Err(e) = process_payload(&state, &payload).await {
                    tracing::warn!(error = %e, "webhook_worker: processing failed");
                }
            }
            Ok(None) => {
                // BRPOP timed out (5s server-side wait, queue was empty) — loop immediately
                tracing::debug!("webhook_worker: BRPOP timeout, no items");
            }
            Err(e) => {
                tracing::warn!(error = %e, "webhook_worker: redis pop failed");
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        }
    }
}

async fn process_payload(state: &AppState, raw: &str) -> Result<(), String> {
    // Try Alchemy Address Activity format first
    let payload: AlchemyPayload =
        serde_json::from_str(raw).map_err(|e| format!("json parse: {e}"))?;

    let activities = payload.event.and_then(|e| e.activity).unwrap_or_default();

    // If no activities, try Alchemy Custom Webhook (GraphQL) format
    if activities.is_empty() {
        // Log first 500 chars of raw payload for debugging format
        let preview = if raw.len() > 500 { &raw[..500] } else { raw };
        tracing::info!(preview = %preview, "webhook_worker: no activities, trying graphql format");
        return process_graphql_payload(state, raw).await;
    }

    tracing::info!(
        activities = activities.len(),
        "webhook_worker: processing payload"
    );

    let mut processed = 0;

    for activity in activities {
        if let Some(log) = activity.log {
            if let Some(event) = parse_log(&activity.hash, &log) {
                // When InvoicePaid confirmed: queue receipt + positive reputation
                // Only trigger side-effects if this is a NEW confirmation (not a duplicate)
                let already_confirmed = state
                    .db
                    .is_event_confirmed(&event.tx_hash, event.log_index)
                    .await
                    .unwrap_or(false);

                // For ANY event type with an invoice_id, try to promote a pending
                // (optimistic) row to confirmed in-place, avoiding duplicates.
                let mut promoted = false;
                if !already_confirmed {
                    if let Some(inv_id) = event.invoice_id {
                        promoted = state
                            .db
                            .confirm_pending_event_for_invoice(
                                inv_id,
                                &event.event_type,
                                &event.tx_hash,
                                event.block_number,
                                event.log_index,
                                event.timestamp,
                                event.payer.as_deref(),
                                event.issuer.as_deref(),
                                event.amount.as_deref(),
                            )
                            .await
                            .unwrap_or(false);
                        if promoted {
                            tracing::info!(
                                invoice_id = inv_id,
                                event_type = %event.event_type,
                                "webhook_worker: promoted pending → confirmed"
                            );
                        }
                    }
                }

                if !already_confirmed && event.event_type == "InvoicePaid" {
                    if let Some(inv_id) = event.invoice_id {
                        tracing::info!(
                            invoice_id = inv_id,
                            tx = %event.tx_hash,
                            "webhook_worker: InvoicePaid confirmed, queuing receipt + feedback"
                        );

                        let receipt_hash =
                            alloy_primitives::keccak256(U256::from(inv_id).to_be_bytes::<32>());
                        let hash_bytes: [u8; 32] = receipt_hash.into();
                        let _ = state
                            .db
                            .insert_pending_receipt(&hash_bytes, Some(inv_id))
                            .await;

                        // Positive reputation feedback (only after confirmed)
                        if let Some(ref payer) = event.payer {
                            if let Ok(payer_addr) = payer.parse::<alloy_primitives::Address>() {
                                let amount = event
                                    .amount
                                    .as_ref()
                                    .and_then(|a| a.parse::<U256>().ok())
                                    .unwrap_or(U256::from(1));
                                let fb = crate::contracts::bay8004::IBay8004::giveFeedbackCall {
                                    subject: payer_addr,
                                    invoiceId: U256::from(inv_id),
                                    amount,
                                };
                                let _ = state
                                    .enqueue_tx(
                                        state.config.bay8004_address,
                                        alloy_sol_types::SolCall::abi_encode(&fb),
                                        5,
                                        "webhook:feedback",
                                    )
                                    .await;
                            }
                        }
                    }
                }

                // If we promoted the pending row, skip the upsert (it's already confirmed)
                // Otherwise upsert as normal for events without a prior pending row
                if !promoted {
                    state
                        .db
                        .upsert_confirmed_event(&event)
                        .await
                        .map_err(|e| e.to_string())?;
                }
                processed += 1;
            } else {
                // Log exists but couldn't be parsed into a known event
                let topics_count = log.topics.as_ref().map(|t| t.len()).unwrap_or(0);
                let data_len = log.data.as_ref().map(|d| d.len()).unwrap_or(0);
                tracing::debug!(
                    topics = topics_count,
                    data_len,
                    tx = ?activity.hash,
                    "webhook_worker: unrecognized log (skipped)"
                );
            }
        } else {
            // Activity has no log field — might be a native transfer
            tracing::debug!(
                tx = ?activity.hash,
                "webhook_worker: activity has no log field (native transfer?)"
            );
        }
    }

    if processed > 0 {
        tracing::info!(processed, "webhook_worker: events confirmed");
    } else {
        tracing::debug!("webhook_worker: payload had no parseable events");
    }

    Ok(())
}

/// Parse an Alchemy log into an IndexedEvent.
fn parse_log(tx_hash: &Option<String>, log: &AlchemyLog) -> Option<IndexedEvent> {
    let topics = log.topics.as_ref()?;
    if topics.is_empty() {
        return None;
    }

    let real_tx_hash = log
        .transaction_hash
        .as_deref()
        .or(tx_hash.as_deref())
        .unwrap_or("unknown");
    let block_number = parse_hex_u64(log.block_number.as_deref().unwrap_or("0x0"));
    let log_index = parse_hex_u32(log.log_index.as_deref().unwrap_or("0x0"));
    let now = now_ts();

    match topics.len() {
        // 4 topics: InvoiceCreated (invoiceId, issuer, payer indexed + amount in data)
        4 => {
            let invoice_id = parse_topic_u64(&topics[1]);
            let issuer = parse_topic_address(&topics[2]);
            let payer = parse_topic_address(&topics[3]);
            let amount = parse_data_u256(log.data.as_deref());

            Some(IndexedEvent {
                event_type: "InvoiceCreated".to_string(),
                block_number,
                tx_hash: real_tx_hash.to_string(),
                log_index,
                timestamp: now,
                invoice_id: Some(invoice_id),
                payer: Some(payer),
                issuer: Some(issuer),
                amount: if amount > U256::ZERO {
                    Some(amount.to_string())
                } else {
                    None
                },
                params: "{}".to_string(),
                status: "pending".to_string(),
            })
        }
        // 3 topics: InvoicePresented/Paid/Overdue (invoiceId, payer indexed)
        3 => {
            let invoice_id = parse_topic_u64(&topics[1]);
            let payer = parse_topic_address(&topics[2]);
            let data = log.data.as_deref().unwrap_or("0x");
            let data_bytes = (data.len().saturating_sub(2)) / 2;

            let (event_type, amount) = if data_bytes >= 64 {
                ("InvoicePaid", Some(parse_data_u256(Some(data)).to_string()))
            } else if data_bytes >= 32 {
                ("InvoicePresented", None)
            } else {
                ("InvoiceOverdue", None)
            };

            Some(IndexedEvent {
                event_type: event_type.to_string(),
                block_number,
                tx_hash: real_tx_hash.to_string(),
                log_index,
                timestamp: now,
                invoice_id: Some(invoice_id),
                payer: Some(payer),
                issuer: None,
                amount,
                params: "{}".to_string(),
                status: "pending".to_string(),
            })
        }
        _ => None,
    }
}

fn parse_hex_u64(hex: &str) -> u64 {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    u64::from_str_radix(s, 16).unwrap_or(0)
}

fn parse_hex_u32(hex: &str) -> u32 {
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    u32::from_str_radix(s, 16).unwrap_or(0)
}

fn parse_topic_u64(topic: &str) -> u64 {
    let s = topic.strip_prefix("0x").unwrap_or(topic);
    let start = s.len().saturating_sub(16);
    u64::from_str_radix(&s[start..], 16).unwrap_or(0)
}

fn parse_topic_address(topic: &str) -> String {
    let s = topic.strip_prefix("0x").unwrap_or(topic);
    let start = s.len().saturating_sub(40);
    format!("0x{}", &s[start..])
}

fn parse_data_u256(data: Option<&str>) -> U256 {
    let hex = data.unwrap_or("0x");
    let s = hex.strip_prefix("0x").unwrap_or(hex);
    if s.len() >= 64 {
        U256::from_str_radix(&s[..64], 16).unwrap_or(U256::ZERO)
    } else {
        U256::ZERO
    }
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

// ─── Alchemy Custom Webhook (GraphQL) format ─────────────────────────────────

#[derive(Deserialize)]
struct GraphQLPayload {
    event: Option<GraphQLEvent>,
}

#[derive(Deserialize)]
struct GraphQLEvent {
    data: Option<GraphQLData>,
}

#[derive(Deserialize)]
struct GraphQLData {
    block: Option<GraphQLBlock>,
}

#[derive(Deserialize)]
struct GraphQLBlock {
    hash: Option<String>,
    number: Option<serde_json::Value>,
    #[allow(dead_code)]
    timestamp: Option<serde_json::Value>,
    logs: Option<Vec<GraphQLLog>>,
}

#[derive(Deserialize)]
struct GraphQLLog {
    data: Option<String>,
    topics: Option<Vec<String>>,
    #[allow(dead_code)]
    index: Option<u32>,
    transaction: Option<GraphQLTransaction>,
}

#[derive(Deserialize)]
struct GraphQLTransaction {
    hash: Option<String>,
}

/// Process Alchemy Custom Webhook (GraphQL) payload format.
async fn process_graphql_payload(state: &AppState, raw: &str) -> Result<(), String> {
    let payload: GraphQLPayload =
        serde_json::from_str(raw).map_err(|e| format!("graphql json parse: {e}"))?;

    let block = payload
        .event
        .and_then(|e| e.data)
        .and_then(|d| d.block)
        .ok_or_else(|| "no block in graphql payload".to_string())?;

    let logs = block.logs.unwrap_or_default();

    if logs.is_empty() {
        tracing::debug!("webhook_worker: graphql payload had no logs");
        return Ok(());
    }

    let block_number = match &block.number {
        Some(serde_json::Value::Number(n)) => n.as_u64().unwrap_or(0),
        Some(serde_json::Value::String(s)) => parse_hex_u64(s),
        _ => 0,
    };

    tracing::info!(
        logs = logs.len(),
        block = block_number,
        "webhook_worker: processing graphql payload"
    );

    let mut processed = 0;

    for log in &logs {
        let topics = match &log.topics {
            Some(t) if !t.is_empty() => t,
            _ => continue,
        };

        let tx_hash = log
            .transaction
            .as_ref()
            .and_then(|t| t.hash.as_deref())
            .unwrap_or("unknown");

        let log_index = log.index.unwrap_or(0);
        let data = log.data.as_deref();
        let now = now_ts();

        let event = match topics.len() {
            // 4 topics: InvoiceCreated (invoiceId, issuer, payer indexed + amount in data)
            4 => {
                let invoice_id = parse_topic_u64(&topics[1]);
                let issuer = parse_topic_address(&topics[2]);
                let payer = parse_topic_address(&topics[3]);
                let amount = parse_data_u256(data);

                Some(IndexedEvent {
                    event_type: "InvoiceCreated".to_string(),
                    block_number,
                    tx_hash: tx_hash.to_string(),
                    log_index,
                    timestamp: now,
                    invoice_id: Some(invoice_id),
                    payer: Some(payer),
                    issuer: Some(issuer),
                    amount: if amount > U256::ZERO {
                        Some(amount.to_string())
                    } else {
                        None
                    },
                    params: "{}".to_string(),
                    status: "pending".to_string(),
                })
            }
            // 3 topics: InvoicePresented/Paid/Overdue
            3 => {
                let invoice_id = parse_topic_u64(&topics[1]);
                let payer = parse_topic_address(&topics[2]);
                let data_str = data.unwrap_or("0x");
                let data_bytes = (data_str.len().saturating_sub(2)) / 2;

                let (event_type, amount) = if data_bytes >= 64 {
                    (
                        "InvoicePaid",
                        Some(parse_data_u256(Some(data_str)).to_string()),
                    )
                } else if data_bytes >= 32 {
                    ("InvoicePresented", None)
                } else {
                    ("InvoiceOverdue", None)
                };

                Some(IndexedEvent {
                    event_type: event_type.to_string(),
                    block_number,
                    tx_hash: tx_hash.to_string(),
                    log_index,
                    timestamp: now,
                    invoice_id: Some(invoice_id),
                    payer: Some(payer),
                    issuer: None,
                    amount,
                    params: "{}".to_string(),
                    status: "pending".to_string(),
                })
            }
            _ => None,
        };

        if let Some(event) = event {
            // Check if already confirmed (idempotency — skip side-effects on duplicates)
            let already_confirmed = state
                .db
                .is_event_confirmed(&event.tx_hash, event.log_index)
                .await
                .unwrap_or(false);

            tracing::info!(
                event_type = %event.event_type,
                invoice_id = ?event.invoice_id,
                tx = %event.tx_hash,
                new = !already_confirmed,
                "webhook_worker: confirmed event"
            );

            // For ANY event type with an invoice_id, try to promote a pending
            // (optimistic) row to confirmed in-place, avoiding duplicates.
            let mut promoted = false;
            if !already_confirmed {
                if let Some(inv_id) = event.invoice_id {
                    promoted = state
                        .db
                        .confirm_pending_event_for_invoice(
                            inv_id,
                            &event.event_type,
                            &event.tx_hash,
                            event.block_number,
                            event.log_index,
                            event.timestamp,
                            event.payer.as_deref(),
                            event.issuer.as_deref(),
                            event.amount.as_deref(),
                        )
                        .await
                        .unwrap_or(false);
                    if promoted {
                        tracing::info!(
                            invoice_id = inv_id,
                            event_type = %event.event_type,
                            "webhook_worker: promoted pending → confirmed (graphql)"
                        );
                    }
                }
            }

            // Side effects for InvoicePaid — ONLY on first confirmation
            if !already_confirmed && event.event_type == "InvoicePaid" {
                if let Some(inv_id) = event.invoice_id {
                    let receipt_hash =
                        alloy_primitives::keccak256(U256::from(inv_id).to_be_bytes::<32>());
                    let hash_bytes: [u8; 32] = receipt_hash.into();
                    let _ = state
                        .db
                        .insert_pending_receipt(&hash_bytes, Some(inv_id))
                        .await;

                    if let Some(ref payer) = event.payer {
                        if let Ok(payer_addr) = payer.parse::<alloy_primitives::Address>() {
                            let amount = event
                                .amount
                                .as_ref()
                                .and_then(|a| a.parse::<U256>().ok())
                                .unwrap_or(U256::from(1));
                            let fb = crate::contracts::bay8004::IBay8004::giveFeedbackCall {
                                subject: payer_addr,
                                invoiceId: U256::from(inv_id),
                                amount,
                            };
                            let _ = state
                                .enqueue_tx(
                                    state.config.bay8004_address,
                                    alloy_sol_types::SolCall::abi_encode(&fb),
                                    5,
                                    "webhook:feedback",
                                )
                                .await;
                        }
                    }
                }
            }

            // If we promoted the pending row, skip the upsert (already confirmed)
            if !promoted {
                state
                    .db
                    .upsert_confirmed_event(&event)
                    .await
                    .map_err(|e| e.to_string())?;
            }
            processed += 1;
        }
    }

    if processed > 0 {
        tracing::info!(
            processed,
            block = block_number,
            "webhook_worker: graphql events confirmed"
        );
    } else {
        tracing::debug!(
            logs = logs.len(),
            "webhook_worker: graphql logs had no parseable events"
        );
    }

    Ok(())
}
