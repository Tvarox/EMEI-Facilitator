/// Alchemy webhook endpoint with HMAC-SHA256 signature verification.
use std::sync::Arc;

use axum::{body::Bytes, extract::State, http::HeaderMap, http::StatusCode};
use hmac::{Hmac, Mac};
use sha2::Sha256;

use crate::state::AppState;

type HmacSha256 = Hmac<Sha256>;

/// POST /emei/webhook — Receive event notifications from Alchemy.
/// Verifies HMAC-SHA256 signature if `ALCHEMY_WEBHOOK_SIGNING_KEY` is configured.
/// Queues valid payloads to Redis for async processing by the webhook worker.
pub async fn handle_webhook(
    State(state): State<Arc<AppState>>,
    headers: HeaderMap,
    body: Bytes,
) -> StatusCode {
    // Verify signature if signing key is configured
    if let Some(ref signing_key) = state.config.webhook_signing_key {
        let signature = match headers.get("x-alchemy-signature") {
            Some(val) => match val.to_str() {
                Ok(s) => s.to_string(),
                Err(_) => return StatusCode::UNAUTHORIZED,
            },
            None => return StatusCode::UNAUTHORIZED,
        };

        if !verify_signature(signing_key, &body, &signature) {
            tracing::warn!("webhook: invalid signature");
            return StatusCode::UNAUTHORIZED;
        }
    }

    let payload = match String::from_utf8(body.to_vec()) {
        Ok(s) => s,
        Err(_) => return StatusCode::BAD_REQUEST,
    };

    match state.redis.push_webhook(&payload).await {
        Ok(_) => {
            tracing::debug!("webhook: queued payload");
            StatusCode::OK
        }
        Err(e) => {
            tracing::error!(error = %e, "webhook: failed to queue");
            StatusCode::INTERNAL_SERVER_ERROR
        }
    }
}

/// Verify Alchemy webhook HMAC-SHA256 signature.
fn verify_signature(signing_key: &str, body: &[u8], signature: &str) -> bool {
    let Ok(mut mac) = HmacSha256::new_from_slice(signing_key.as_bytes()) else {
        return false;
    };
    mac.update(body);

    // Alchemy sends the signature as a hex-encoded HMAC
    let Ok(expected) = hex::decode(signature) else {
        return false;
    };

    mac.verify_slice(&expected).is_ok()
}
