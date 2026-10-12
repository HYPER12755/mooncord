//! Token storage for Supabase auth session.
//!
//! Unlike the Discord backend which stores a single opaque string,
//! Supabase uses a JWT access token paired with a refresh token.
//! This struct carries both so the backend can transparently renew
//! expired access tokens.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// A Supabase auth session token pair.
///
/// `access_token` is a JWT signed by Supabase Auth.
/// `refresh_token` is an opaque string that can mint a new access token.
/// `expires_at` is the unix timestamp when `access_token` expires.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionTokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_at: u64,
}

impl SessionTokens {
    pub fn new(access_token: String, refresh_token: String, expires_at: u64) -> Self {
        Self {
            access_token,
            refresh_token,
            expires_at,
        }
    }

    /// Returns `true` if the access token has expired (with a 2-minute safety margin).
    pub fn is_expired(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        now >= self.expires_at.saturating_sub(120)
    }

    /// Returns the bearer token to send in `Authorization: Bearer <token>`.
    pub fn bearer(&self) -> &str {
        &self.access_token
    }

    /// Construct a `SessionTokens` from a JWT access token string.
    ///
    /// Extracts the expiry (`exp`) claim from the JWT payload. The refresh
    /// token is left empty because this method is used for tokens obtained
    /// out-of-band (e.g. `CONCORD_TOKEN` env var) where no refresh token is
    /// available.
    pub fn from_jwt_string(access_token: &str) -> Self {
        let exp = extract_jwt_exp(access_token).unwrap_or(0);
        Self::new(
            access_token.to_string(),
            String::new(),
            exp,
        )
    }
}

/// Extract the `exp` (expiry) claim from a JWT payload.
fn extract_jwt_exp(token: &str) -> Option<u64> {
    use base64::Engine;
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(parts[1])
        .ok()?;
    let claims: serde_json::Value = serde_json::from_slice(&payload).ok()?;
    claims.get("exp").and_then(|v| v.as_u64())
}
