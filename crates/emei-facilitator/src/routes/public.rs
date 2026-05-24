//! Public read-only endpoints for the live demo dashboard.
//!
//! All endpoints are unauthenticated GET requests under `/emei/public/`.
//! They read from the existing SQLite indexer and chain provider.

use std::sync::Arc;

use alloy_primitives::{Address, U256};
use alloy_sol_types::SolCall;
use axum::{
    extract::{Query, State},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};

use crate::{
    contracts::{bay8004::IBay8004, mandate::IEMEIMandate, settlement::IEMEISettlement},
    error::EmeiError,
    state::AppState,
};

/// Build the public sub-router (mounted at `/emei/public`).
pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/stats", get(get_stats))
        .route("/events", get(get_events))
        .route("/agents", get(get_agents))
        .route("/mandates", get(get_mandates))
}

// ─── Response types ──────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct StatsResponse {
    pub totals: EventTotals,
    pub gmv_settled_musd: String,
    pub active_mandates: i64,
    pub vault_tvl_musd: String,
    pub latest_block: i64,
    pub latest_receipt_root: Option<String>,
    pub latest_receipt_at: Option<i64>,
    pub chain_id: u64,
    pub network: String,
}

#[derive(Serialize, Default)]
pub struct EventTotals {
    pub invoices_issued: i64,
    pub invoices_presented: i64,
    pub invoices_paid: i64,
    pub invoices_overdue: i64,
    pub mandates_created: i64,
    pub mandates_revoked: i64,
    pub receipts_anchored: i64,
    pub settlements: i64,
}

#[derive(Serialize)]
pub struct EventsResponse {
    pub events: Vec<PublicEvent>,
    pub next_before: Option<i64>,
}

#[derive(Serialize)]
pub struct PublicEvent {
    #[serde(rename = "type")]
    pub event_type: String,
    pub block: u64,
    pub tx_hash: String,
    pub log_index: u32,
    pub timestamp: u64,
    pub invoice_id: Option<u64>,
    pub issuer: Option<String>,
    pub payer: Option<String>,
    pub amount_musd: Option<String>,
    pub category: Option<String>,
}

#[derive(Serialize)]
pub struct AgentsResponse {
    pub agents: Vec<AgentInfo>,
}

#[derive(Serialize)]
pub struct AgentInfo {
    pub address: String,
    pub label: String,
    pub vault_balance_musd: String,
    pub reputation_score: u64,
    pub invoices_issued: i64,
    pub invoices_paid_to_them: i64,
    pub invoices_paid_by_them: i64,
    pub active_mandates: i64,
}

#[derive(Serialize)]
pub struct MandatesResponse {
    pub mandates: Vec<MandateInfo>,
}

#[derive(Serialize)]
pub struct MandateInfo {
    pub mandate_id: u64,
    pub payer: String,
    pub payer_label: String,
    pub spend_cap_musd: String,
    pub remaining_cap_musd: String,
    pub spent_musd: String,
    pub approved_counterparties: Vec<String>,
    pub approved_categories: Vec<String>,
    pub valid_from: u64,
    pub valid_until: u64,
    pub status: String,
}

// ─── Query params ────────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct EventsQuery {
    pub limit: Option<i64>,
    pub before: Option<i64>,
}

// ─── Handlers ────────────────────────────────────────────────────────────────

/// GET /emei/public/stats — Aggregate protocol statistics.
pub async fn get_stats(
    State(state): State<Arc<AppState>>,
) -> Result<Json<StatsResponse>, EmeiError> {
    // Count events by type
    let counts = state.db.count_events_by_type().await?;
    let mut totals = EventTotals::default();

    for (event_type, count) in &counts {
        match event_type.as_str() {
            "InvoiceCreated" | "InvoiceIssued" => totals.invoices_issued += count,
            "InvoicePresented" => totals.invoices_presented += count,
            "InvoicePaid" => totals.invoices_paid += count,
            "InvoiceOverdue" => totals.invoices_overdue += count,
            "MandateCreated" => totals.mandates_created += count,
            "MandateRevoked" => totals.mandates_revoked += count,
            "MerkleRootPosted" => totals.receipts_anchored += count,
            "SettlementExecuted" => totals.settlements += count,
            _ => {}
        }
    }

    // GMV settled — sum of InvoicePaid amounts (fallback if SettlementExecuted not emitted)
    let gmv_wei = state.db.sum_amount_for_type("InvoicePaid").await?;
    let gmv_musd = wei_to_musd_string(gmv_wei);

    // Active mandates (best effort: created - revoked)
    let active_mandates = totals.mandates_created - totals.mandates_revoked;

    // Vault TVL — sum vault balances for known agents
    let vault_tvl = get_vault_tvl(&state).await;
    let vault_tvl_musd = wei_to_musd_string(vault_tvl);

    // Latest block from events
    let latest_block = state.db.latest_block().await?.unwrap_or(0);

    // Latest receipt root
    let (latest_receipt_root, latest_receipt_at) = match state.db.latest_receipt_event().await? {
        Some((params, ts)) => {
            // Try to extract root from params JSON
            let root = serde_json::from_str::<serde_json::Value>(&params)
                .ok()
                .and_then(|v| v.get("root").and_then(|r| r.as_str().map(String::from)))
                .unwrap_or_else(|| params.clone());
            (Some(root), Some(ts))
        }
        None => (None, None),
    };

    Ok(Json(StatsResponse {
        totals,
        gmv_settled_musd: gmv_musd,
        active_mandates,
        vault_tvl_musd,
        latest_block,
        latest_receipt_root,
        latest_receipt_at,
        chain_id: 5003,
        network: "Mantle Sepolia".to_string(),
    }))
}

/// GET /emei/public/events — Paginated event feed.
pub async fn get_events(
    State(state): State<Arc<AppState>>,
    Query(params): Query<EventsQuery>,
) -> Result<Json<EventsResponse>, EmeiError> {
    let limit = params.limit.unwrap_or(20).min(100).max(1);
    let before_block = params.before;

    let events = state.db.recent_events(limit, before_block).await?;

    let next_before = events.last().map(|e| e.block_number as i64);

    let public_events: Vec<PublicEvent> = events
        .into_iter()
        .map(|e| {
            let amount_musd = e
                .amount
                .as_ref()
                .map(|a| wei_to_musd_string(a.parse::<u128>().unwrap_or(0)));
            // Try to extract category from params
            let category = serde_json::from_str::<serde_json::Value>(&e.params)
                .ok()
                .and_then(|v| v.get("category").and_then(|c| c.as_str().map(String::from)));

            PublicEvent {
                event_type: e.event_type,
                block: e.block_number,
                tx_hash: e.tx_hash,
                log_index: e.log_index,
                timestamp: e.timestamp,
                invoice_id: e.invoice_id,
                issuer: e.issuer,
                payer: e.payer,
                amount_musd,
                category,
            }
        })
        .collect();

    Ok(Json(EventsResponse {
        events: public_events,
        next_before,
    }))
}

/// GET /emei/public/agents — Known agent addresses with enrichment.
pub async fn get_agents(
    State(state): State<Arc<AppState>>,
) -> Result<Json<AgentsResponse>, EmeiError> {
    let agents_config = parse_demo_agents();

    let mut agents = Vec::new();

    for (label, address) in agents_config {
        // Get vault balance
        let vault_balance = get_vault_balance_for(&state, &address).await;
        let vault_balance_musd = wei_to_musd_string(vault_balance);

        // Get reputation score
        let reputation_score = get_reputation_for(&state, &address).await;

        // Count events as issuer
        let issuer_counts = state
            .db
            .count_events_for_issuer(&address)
            .await
            .unwrap_or_default();
        let invoices_issued = issuer_counts
            .iter()
            .filter(|(t, _)| t == "InvoiceCreated" || t == "InvoiceIssued")
            .map(|(_, c)| c)
            .sum::<i64>();

        // Count InvoicePaid events where this address is the issuer (paid to them)
        let invoices_paid_to_them = issuer_counts
            .iter()
            .filter(|(t, _)| t == "InvoicePaid")
            .map(|(_, c)| c)
            .sum::<i64>();

        // Count events as payer
        let payer_counts = state
            .db
            .count_events_for_payer(&address)
            .await
            .unwrap_or_default();
        let invoices_paid_by_them = payer_counts
            .iter()
            .filter(|(t, _)| t == "InvoicePaid")
            .map(|(_, c)| c)
            .sum::<i64>();

        // Active mandates (created - revoked as payer)
        let mandates_created = payer_counts
            .iter()
            .filter(|(t, _)| t == "MandateCreated")
            .map(|(_, c)| c)
            .sum::<i64>();
        let mandates_revoked = payer_counts
            .iter()
            .filter(|(t, _)| t == "MandateRevoked")
            .map(|(_, c)| c)
            .sum::<i64>();
        let active_mandates = mandates_created - mandates_revoked;

        agents.push(AgentInfo {
            address,
            label,
            vault_balance_musd,
            reputation_score,
            invoices_issued,
            invoices_paid_to_them,
            invoices_paid_by_them,
            active_mandates,
        });
    }

    Ok(Json(AgentsResponse { agents }))
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

/// GET /emei/public/mandates — Active mandates with real-time cap usage.
pub async fn get_mandates(
    State(state): State<Arc<AppState>>,
) -> Result<Json<MandatesResponse>, EmeiError> {
    let agents = parse_demo_agents();
    let mut mandates = Vec::new();

    // For each known agent (as payer), query their mandates
    for (label, address) in &agents {
        let addr: Address = match address.parse() {
            Ok(a) => a,
            Err(_) => continue,
        };

        // Get mandate IDs for this payer
        let calldata = IEMEIMandate::getMandatesByPayerCall { payer: addr }.abi_encode();
        let result = match state
            .chain
            .call(state.config.mandate_address, calldata.into())
            .await
        {
            Ok(r) => r,
            Err(_) => continue,
        };

        let mandate_ids = match IEMEIMandate::getMandatesByPayerCall::abi_decode_returns(&result) {
            Ok(ids) => ids,
            Err(_) => continue,
        };

        for mandate_id_u256 in mandate_ids.iter() {
            let mandate_id: u64 = (*mandate_id_u256).try_into().unwrap_or(0);
            if mandate_id == 0 {
                continue;
            }

            // Fetch mandate details
            let m_calldata = IEMEIMandate::getMandateCall {
                mandateId: U256::from(mandate_id),
            }
            .abi_encode();

            let m_result = match state
                .chain
                .call(state.config.mandate_address, m_calldata.into())
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };

            let mandate = match IEMEIMandate::getMandateCall::abi_decode_returns(&m_result) {
                Ok(m) => m,
                Err(_) => continue,
            };

            let spend_cap_wei: u128 = mandate.spendCap.try_into().unwrap_or(0);
            let remaining_wei: u128 = mandate.remainingCap.try_into().unwrap_or(0);
            let spent_wei = spend_cap_wei.saturating_sub(remaining_wei);

            let status_str = match mandate.status {
                0 => "active",
                1 => "revoked",
                _ => "unknown",
            };

            let counterparties: Vec<String> = mandate
                .approvedCounterparties
                .iter()
                .map(|a| format!("0x{}", hex::encode(a)))
                .collect();

            let valid_from: u64 = mandate.validFrom.try_into().unwrap_or(0);
            let valid_until: u64 = mandate.validUntil.try_into().unwrap_or(0);

            mandates.push(MandateInfo {
                mandate_id,
                payer: address.clone(),
                payer_label: label.clone(),
                spend_cap_musd: wei_to_musd_string(spend_cap_wei),
                remaining_cap_musd: wei_to_musd_string(remaining_wei),
                spent_musd: wei_to_musd_string(spent_wei),
                approved_counterparties: counterparties,
                approved_categories: mandate.approvedCategories.clone(),
                valid_from,
                valid_until,
                status: status_str.to_string(),
            });
        }
    }

    Ok(Json(MandatesResponse { mandates }))
}

// ─── Helpers (internal) ──────────────────────────────────────────────────────

/// Convert wei (u128) to a human-readable mUSD string with 2 decimal places.
fn wei_to_musd_string(wei: u128) -> String {
    let whole = wei / 1_000_000_000_000_000_000;
    let frac = (wei % 1_000_000_000_000_000_000) / 10_000_000_000_000_000; // 2 decimal places
    format!("{}.{:02}", whole, frac)
}

/// Parse the DEMO_AGENTS env var (format: "name:0xaddr,name2:0xaddr2").
/// Falls back to an empty list if unset. Lowercases addresses for DB matching.
fn parse_demo_agents() -> Vec<(String, String)> {
    let raw = std::env::var("DEMO_AGENTS").unwrap_or_default();
    if raw.is_empty() {
        return Vec::new();
    }
    raw.split(',')
        .filter_map(|entry| {
            let parts: Vec<&str> = entry.trim().splitn(2, ':').collect();
            if parts.len() == 2 {
                Some((parts[0].to_string(), parts[1].to_lowercase()))
            } else {
                None
            }
        })
        .collect()
}

/// Get the total vault TVL by summing vault balances for known agents.
async fn get_vault_tvl(state: &AppState) -> u128 {
    let agents = parse_demo_agents();
    let mut total: u128 = 0;
    for (_, address) in agents {
        total += get_vault_balance_for(state, &address).await;
    }
    total
}

/// Get vault balance for a single address from the chain.
async fn get_vault_balance_for(state: &AppState, address: &str) -> u128 {
    let addr: Address = match address.parse() {
        Ok(a) => a,
        Err(_) => return 0,
    };

    let calldata = IEMEISettlement::getVaultBalanceCall { payee: addr }.abi_encode();
    match state
        .chain
        .call(state.config.settlement_address, calldata.into())
        .await
    {
        Ok(result) => {
            IEMEISettlement::getVaultBalanceCall::abi_decode_returns(&result)
                .ok()
                .map(|v| {
                    let bytes = v.to_be_bytes::<32>();
                    // Convert U256 to u128 (take lower 16 bytes)
                    u128::from_be_bytes(bytes[16..32].try_into().unwrap_or([0u8; 16]))
                })
                .unwrap_or(0)
        }
        Err(_) => 0,
    }
}

/// Get reputation score for a single address from the chain.
async fn get_reputation_for(state: &AppState, address: &str) -> u64 {
    let addr: Address = match address.parse() {
        Ok(a) => a,
        Err(_) => return 0,
    };

    let calldata = IBay8004::scoreOfCall { account: addr }.abi_encode();
    match state
        .chain
        .call(state.config.bay8004_address, calldata.into())
        .await
    {
        Ok(result) => IBay8004::scoreOfCall::abi_decode_returns(&result)
            .ok()
            .map(|v| v.try_into().unwrap_or(0u64))
            .unwrap_or(0),
        Err(_) => 0,
    }
}
