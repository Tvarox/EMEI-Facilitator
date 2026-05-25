/// Background services for the EMEI facilitator.
pub mod batcher;
pub mod collector;
pub mod indexer;
pub mod scanner;
pub mod tx_reaper;
pub mod tx_sender;
pub mod webhook_worker;

use std::sync::Arc;

use tokio::task::JoinHandle;

use crate::state::AppState;

/// Spawn all background services.
pub fn start_services(state: Arc<AppState>) -> Vec<JoinHandle<()>> {
    let cancel = state.cancel.clone();

    let mut handles = vec![
        tokio::spawn(batcher::receipt_batcher(state.clone(), cancel.clone())),
        tokio::spawn(collector::auto_collector(state.clone(), cancel.clone())),
        tokio::spawn(scanner::overdue_scanner(state.clone(), cancel.clone())),
        tokio::spawn(indexer::event_indexer(state.clone(), cancel.clone())),
        tokio::spawn(webhook_worker::webhook_worker(
            state.clone(),
            cancel.clone(),
        )),
        tokio::spawn(tx_reaper::tx_reaper(state.clone(), cancel.clone())),
    ];

    // Spawn TX sender workers (one per wallet key)
    let wallet_keys = state.config.hot_wallet_keys.clone();
    let sender_handles = tx_sender::spawn_tx_senders(state.clone(), wallet_keys, cancel);
    handles.extend(sender_handles);

    handles
}
