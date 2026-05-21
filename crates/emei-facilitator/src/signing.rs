use alloy_primitives::B256;
use alloy_signer_local::PrivateKeySigner;
use axum::extract::FromRequestParts;
use axum::http::request::Parts;

use crate::error::EmeiError;

/// Axum extractor that reads a private key from the `X-Private-Key` header.
pub struct UserSigner(pub PrivateKeySigner);

impl<S> FromRequestParts<S> for UserSigner
where
    S: Send + Sync,
{
    type Rejection = EmeiError;

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let key_hex = parts
            .headers
            .get("X-Private-Key")
            .ok_or(EmeiError::MissingAuth)?
            .to_str()
            .map_err(|_| EmeiError::InvalidAuth("non-ASCII header value".into()))?;

        let stripped = key_hex.strip_prefix("0x").unwrap_or(key_hex);
        let key_bytes = hex::decode(stripped)
            .map_err(|_| EmeiError::InvalidAuth("invalid hex in X-Private-Key".into()))?;

        if key_bytes.len() != 32 {
            return Err(EmeiError::InvalidAuth("key must be 32 bytes".into()));
        }

        let b256 = B256::from_slice(&key_bytes);
        let signer = PrivateKeySigner::from_bytes(&b256)
            .map_err(|e| EmeiError::InvalidAuth(format!("invalid private key: {e}")))?;

        Ok(UserSigner(signer))
    }
}
