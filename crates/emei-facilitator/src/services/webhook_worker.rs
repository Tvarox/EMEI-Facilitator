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
    tracing::info!("webhook_worker: started");

    loop {
        tokio::select! {
            _ = cancel.cancelled() => {
                tracing::info!("webhook_worker: shutting down");
                break;
            }
            result = state.redis.pop_webhook() => {
                match result {
                    Ok(Some(payload)) => {
                        if let Err(e) = process_payload(&state, &payload).await {
                            tracing::warn!(error = %e, "webhook_worker: processing failed");
                        }
                    }
                    Ok(None) => {} // Timeout, loop again
                    Err(e) => {
                        tracing::warn!(error = %e, "webhook_worker: redis pop failed");
                        tokio::time::sleep(Duration::from_secs(5)).await;
                    }
                }
            }
        }
    }
}

async fn process_payload(state: &AppState, raw: &str) -> Result<(), String> {
    let payload: AlchemyPayload =
        serde_json::from_str(raw).map_err(|e| format!("json parse: {e}"))?;

    let activities = payload.event.and_then(|e| e.activity).unwrap_or_default();

    let mut processed = 0;

    for activity in activities {
        if let Some(log) = activity.log {
            if let Some(event) = parse_log(&activity.hash, &log) {
                // When InvoicePaid confirmed: queue receipt + positive reputation
                if event.event_type == "InvoicePaid" {
                    if let Some(inv_id) = event.invoice_id {
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
                                    .chain
                                    .send_hot(
                                        state.config.bay8004_address,
                                        alloy_sol_types::SolCall::abi_encode(&fb).into(),
                                        &state.redis,
                                    )
                                    .await;
                            }
                        }
                    }
                }

                state
                    .db
                    .upsert_confirmed_event(&event)
                    .await
                    .map_err(|e| e.to_string())?;
                processed += 1;
            }
        }
    }

    if processed > 0 {
        tracing::info!(processed, "webhook_worker: events confirmed");
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
