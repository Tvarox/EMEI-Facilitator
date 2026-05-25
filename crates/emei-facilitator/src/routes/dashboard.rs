/// Internal ops dashboard — serves a live HTML page + JSON API for system state.
use std::sync::Arc;

use axum::{
    extract::State,
    http::{header, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Serialize;

use crate::state::AppState;

#[derive(Serialize)]
pub struct SystemStatus {
    pub uptime_secs: u64,
    pub redis_connected: bool,
    pub webhook_queue_len: u64,
    pub receipt_queue_db: usize,
    pub receipt_queue_mem: usize,
    pub pending_txs: usize,
    pub total_events: Vec<(String, i64)>,
    pub confirmed_events: i64,
    pub pending_events: i64,
    pub latest_block: i64,
    pub webhook_signing_enabled: bool,
}

/// GET /emei/ops/status — JSON system internals
pub async fn ops_status(
    State(state): State<Arc<AppState>>,
) -> Result<Json<SystemStatus>, StatusCode> {
    let uptime = state.started_at.elapsed().as_secs();
    let redis_ok = state.redis.ping().await;
    let webhook_q = state.redis.webhook_queue_len().await.unwrap_or(0);
    let receipt_db = state.db.count_pending_receipts().await.unwrap_or(0);
    let receipt_mem = state.receipt_queue.len().await;
    let pending_txs = state
        .db
        .get_stale_pending_txs(0)
        .await
        .map(|v| v.len())
        .unwrap_or(0);
    let event_counts = state.db.count_events_by_type().await.unwrap_or_default();
    let latest_block = state
        .db
        .latest_block()
        .await
        .unwrap_or(Some(0))
        .unwrap_or(0);

    // Count confirmed vs pending
    let confirmed = state
        .db
        .count_events_by_status("confirmed")
        .await
        .unwrap_or(0);
    let pending = state
        .db
        .count_events_by_status("pending")
        .await
        .unwrap_or(0);

    Ok(Json(SystemStatus {
        uptime_secs: uptime,
        redis_connected: redis_ok,
        webhook_queue_len: webhook_q,
        receipt_queue_db: receipt_db,
        receipt_queue_mem: receipt_mem,
        pending_txs,
        total_events: event_counts,
        confirmed_events: confirmed,
        pending_events: pending,
        latest_block,
        webhook_signing_enabled: state.config.webhook_signing_key.is_some(),
    }))
}

/// GET /emei/ops — HTML dashboard (auto-refreshes every 5s)
pub async fn ops_dashboard() -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        DASHBOARD_HTML,
    )
}

/// POST /emei/ops/reset — Truncate all tables and start fresh.
pub async fn ops_reset(State(state): State<Arc<AppState>>) -> StatusCode {
    match state.db.truncate_all().await {
        Ok(_) => {
            tracing::info!("ops: database truncated");
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!(error = %e, "ops: truncate failed");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>EMEI Ops Dashboard</title>
<meta name="viewport" content="width=device-width,initial-scale=1">
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',Roboto,monospace;background:#0d1117;color:#c9d1d9;padding:20px}
h1{color:#58a6ff;margin-bottom:16px;font-size:1.4em}
.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(280px,1fr));gap:16px;margin-bottom:24px}
.card{background:#161b22;border:1px solid #30363d;border-radius:8px;padding:16px}
.card h2{font-size:0.85em;color:#8b949e;text-transform:uppercase;letter-spacing:0.5px;margin-bottom:8px}
.card .value{font-size:1.8em;font-weight:700;color:#f0f6fc}
.card .value.green{color:#3fb950}
.card .value.yellow{color:#d29922}
.card .value.red{color:#f85149}
.table{width:100%;border-collapse:collapse;margin-top:8px}
.table th,.table td{text-align:left;padding:6px 10px;border-bottom:1px solid #21262d;font-size:0.85em}
.table th{color:#8b949e;font-weight:600}
.table td{color:#c9d1d9}
.badge{display:inline-block;padding:2px 8px;border-radius:12px;font-size:0.75em;font-weight:600}
.badge.ok{background:#1f3d2a;color:#3fb950}
.badge.warn{background:#3d2e1a;color:#d29922}
.badge.err{background:#3d1a1a;color:#f85149}
#events{max-height:400px;overflow-y:auto}
.event-row{display:flex;gap:12px;padding:6px 0;border-bottom:1px solid #21262d;font-size:0.82em}
.event-type{min-width:120px;font-weight:600}
.event-type.created{color:#58a6ff}
.event-type.presented{color:#d29922}
.event-type.paid{color:#3fb950}
.event-type.overdue{color:#f85149}
.ts{color:#8b949e;min-width:80px}
.refresh{color:#8b949e;font-size:0.75em;margin-top:8px}
</style>
</head>
<body>
<h1>⚡ EMEI Facilitator — Ops Dashboard</h1>
<div class="grid" id="cards"></div>
<div class="card" style="margin-bottom:16px">
<h2>Event Counts by Type</h2>
<table class="table" id="event-table"><thead><tr><th>Type</th><th>Count</th></tr></thead><tbody></tbody></table>
</div>
<div class="card">
<h2>Recent Events (last 20)</h2>
<div id="events"></div>
</div>
<p class="refresh">Auto-refreshes every 5s</p>
<script>
async function fetchStatus(){
  try{
    const r=await fetch('/emei/ops/status');
    const d=await r.json();
    renderCards(d);
    renderEventTable(d.total_events);
  }catch(e){console.error(e)}
}
async function fetchEvents(){
  try{
    const r=await fetch('/emei/public/events?limit=20');
    const d=await r.json();
    renderEvents(d.events||[]);
  }catch(e){console.error(e)}
}
function renderCards(d){
  const uptime=formatUptime(d.uptime_secs);
  const cards=[
    {label:'Uptime',value:uptime,cls:''},
    {label:'Redis',value:d.redis_connected?'Connected':'DOWN',cls:d.redis_connected?'green':'red'},
    {label:'Webhook Queue',value:d.webhook_queue_len,cls:d.webhook_queue_len>10?'yellow':'green'},
    {label:'Receipt Queue (DB)',value:d.receipt_queue_db,cls:''},
    {label:'Receipt Queue (Mem)',value:d.receipt_queue_mem,cls:''},
    {label:'Pending Txs',value:d.pending_txs,cls:d.pending_txs>5?'yellow':'green'},
    {label:'Confirmed Events',value:d.confirmed_events,cls:'green'},
    {label:'Pending Events',value:d.pending_events,cls:d.pending_events>0?'yellow':'green'},
    {label:'Latest Block',value:d.latest_block,cls:''},
    {label:'Webhook Signing',value:d.webhook_signing_enabled?'Enabled':'Disabled',cls:d.webhook_signing_enabled?'green':'yellow'},
  ];
  document.getElementById('cards').innerHTML=cards.map(c=>`<div class="card"><h2>${c.label}</h2><div class="value ${c.cls}">${c.value}</div></div>`).join('');
}
function renderEventTable(events){
  const tbody=document.querySelector('#event-table tbody');
  tbody.innerHTML=events.map(([type,count])=>`<tr><td>${type}</td><td>${count}</td></tr>`).join('');
}
function renderEvents(events){
  const el=document.getElementById('events');
  el.innerHTML=events.map(e=>{
    const cls=e.type.includes('Created')?'created':e.type.includes('Presented')?'presented':e.type.includes('Paid')?'paid':e.type.includes('Overdue')?'overdue':'';
    const ts=new Date(e.timestamp*1000).toLocaleTimeString();
    const inv=e.invoice_id?`#${e.invoice_id}`:'';
    const amt=e.amount_musd?`${e.amount_musd} mUSD`:'';
    return `<div class="event-row"><span class="ts">${ts}</span><span class="event-type ${cls}">${e.type}</span><span>${inv}</span><span>${amt}</span><span style="color:#8b949e">${(e.tx_hash||'').slice(0,14)}...</span></div>`;
  }).join('');
}
function formatUptime(s){
  const h=Math.floor(s/3600);const m=Math.floor((s%3600)/60);const sec=s%60;
  return h>0?`${h}h ${m}m`:`${m}m ${sec}s`;
}
fetchStatus();fetchEvents();
setInterval(()=>{fetchStatus();fetchEvents()},5000);
</script>
</body>
</html>"#;
