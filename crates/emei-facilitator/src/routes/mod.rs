/// Main route definitions for the EMEI Facilitator API.
use std::sync::Arc;

use axum::{
    routing::{delete, get, post},
    Router,
};

use crate::state::AppState;

pub mod dashboard;
pub mod health;
pub mod identity;
pub mod invoice;
pub mod mandate;
pub mod paylink;
pub mod public;
pub mod query;
pub mod receipt;
pub mod webhook;
pub mod withdraw;

/// All routes are prefixed with `/emei/` and grouped by domain.
pub fn emei_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Invoice lifecycle
        .route("/emei/invoice", post(invoice::create_invoice))
        .route("/emei/present", post(invoice::present_invoice))
        .route("/emei/pay", post(invoice::pay_invoice))
        .route("/emei/collect", post(invoice::collect_invoice))
        // Mandate management
        .route("/emei/mandate", post(mandate::create_mandate))
        .route("/emei/mandate/{id}", delete(mandate::revoke_mandate))
        // Query endpoints
        .route("/emei/invoice/{id}", get(query::get_invoice))
        .route("/emei/statement", get(query::get_statement))
        .route("/emei/reputation/{address}", get(query::get_reputation))
        .route("/emei/balance/{address}", get(query::get_balance))
        // Receipt verification
        .route("/emei/verify/{id}", get(receipt::verify_receipt))
        // Identity registration
        .route("/emei/register", post(identity::register_identity))
        // Withdrawal
        .route("/emei/withdraw", post(withdraw::withdraw_funds))
        // Pay-link (present-and-pay fallback)
        .route("/emei/paylink/{id}", get(paylink::get_paylink))
        // Public dashboard endpoints (read-only, no auth)
        .nest("/emei/public", public::router())
        // Ops dashboard (HTML + JSON)
        .route("/emei/ops", get(dashboard::ops_dashboard))
        .route("/emei/ops/status", get(dashboard::ops_status))
        .route("/emei/ops/reset", post(dashboard::ops_reset))
        // Health check
        .route("/health", get(health::health_check))
        // Webhook (Alchemy event notifications)
        .route("/emei/webhook", post(webhook::handle_webhook))
}
