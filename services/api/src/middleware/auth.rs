//! Mini App `initData` validation middleware.
//!
//! Telegram delivers a signed query string to the Mini App; we forward it to
//! the API as the `X-Telegram-Init-Data` header. The signature is HMAC-SHA256
//! using a secret derived from the bot token (see Telegram docs:
//! <https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app>).
//!
//! On success the validated `TelegramUserId` is inserted into the request
//! extensions so downstream handlers can pull it via `request.extensions()`.

use std::sync::Arc;

use axum::extract::{Request, State};
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;
use chrono::{DateTime, Utc};
use domain::TelegramUserId;
use hmac::{Hmac, KeyInit, Mac};
use rust_utils::secret::Secret;
use serde::Deserialize;
use sha2::Sha256;
use thiserror::Error;
use tracing::warn;

const HEADER: &str = "X-Telegram-Init-Data";
const MAX_AGE_SECS: i64 = 24 * 3600;
const FUTURE_SKEW_SECS: i64 = 60;

type HmacSha256 = Hmac<Sha256>;

/// Pre-derived HMAC secret key. Computed once at startup from the bot token
/// and kept in middleware state so each request only does the inner HMAC pass.
pub type SecretKey = Secret<Vec<u8>>;

/// Causes for which an `initData` payload is rejected. Each variant maps to
/// HTTP 401 at the middleware boundary; the variant name is logged via
/// `tracing::warn!` so operators can tell apart "stale token" from "forged
/// token" without leaking the rejection reason to the caller.
#[derive(Debug, Error)]
enum AuthError {
    #[error("missing X-Telegram-Init-Data header")]
    MissingHeader,
    #[error("missing required field: {0}")]
    MissingField(&'static str),
    #[error("malformed query string")]
    MalformedQuery,
    #[error("hash is not valid hex")]
    InvalidHashEncoding,
    #[error("hash mismatch")]
    InvalidHash,
    #[error("auth_date is not a valid integer")]
    InvalidAuthDate,
    #[error("auth_date older than {MAX_AGE_SECS}s")]
    Expired,
    #[error("auth_date more than {FUTURE_SKEW_SECS}s in the future")]
    Future,
    #[error("user field could not be parsed as JSON: {0}")]
    InvalidUser(String),
}

#[derive(Debug, Deserialize)]
struct UserField {
    id: i64,
}

/// Derive the per-process HMAC secret key from the bot token. Done once at
/// startup so the per-request middleware only runs the inner HMAC pass.
pub fn derive_secret_key(bot_token: &str) -> SecretKey {
    let mut mac = HmacSha256::new_from_slice(b"WebAppData").expect("HMAC accepts any key size");
    mac.update(bot_token.as_bytes());
    Secret::new(mac.finalize().into_bytes().to_vec())
}

/// Verify a Telegram WebApp `initData` query string and extract the user id.
///
/// `secret_key` must be produced by [`derive_secret_key`]. `now` is injected
/// so unit tests can pin the clock.
fn verify_init_data_inner(
    init_data: &str,
    secret_key: &[u8],
    now: DateTime<Utc>,
) -> Result<TelegramUserId, AuthError> {
    let mut hash: Option<String> = None;
    let mut pairs: Vec<(String, String)> = Vec::new();

    for raw_pair in init_data.split('&') {
        if raw_pair.is_empty() {
            continue;
        }
        let (k, v) = raw_pair.split_once('=').ok_or(AuthError::MalformedQuery)?;
        let key = urlencoding::decode(k)
            .map_err(|_| AuthError::MalformedQuery)?
            .into_owned();
        let value = urlencoding::decode(v)
            .map_err(|_| AuthError::MalformedQuery)?
            .into_owned();

        match key.as_str() {
            "hash" => hash = Some(value),
            _ => pairs.push((key, value)),
        }
    }

    let hash = hash.ok_or(AuthError::MissingField("hash"))?;

    pairs.sort_by(|a, b| a.0.cmp(&b.0));
    let data_check_string = pairs
        .iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut mac = HmacSha256::new_from_slice(secret_key).expect("HMAC accepts any key size");
    mac.update(data_check_string.as_bytes());

    let expected_bytes = hex::decode(&hash).map_err(|_| AuthError::InvalidHashEncoding)?;
    mac.verify_slice(&expected_bytes)
        .map_err(|_| AuthError::InvalidHash)?;

    let auth_date_str = pairs
        .iter()
        .find_map(|(k, v)| (k == "auth_date").then_some(v.as_str()))
        .ok_or(AuthError::MissingField("auth_date"))?;
    let user_json = pairs
        .iter()
        .find_map(|(k, v)| (k == "user").then_some(v.as_str()))
        .ok_or(AuthError::MissingField("user"))?;

    let auth_date_secs: i64 = auth_date_str
        .parse()
        .map_err(|_| AuthError::InvalidAuthDate)?;
    let delta = now.timestamp() - auth_date_secs;
    if delta > MAX_AGE_SECS {
        return Err(AuthError::Expired);
    }
    if delta < -FUTURE_SKEW_SECS {
        return Err(AuthError::Future);
    }

    let user: UserField =
        serde_json::from_str(user_json).map_err(|e| AuthError::InvalidUser(e.to_string()))?;
    Ok(TelegramUserId(user.id))
}

/// Axum middleware. Reads the header, verifies, injects `TelegramUserId`
/// into request extensions.
pub async fn verify_init_data(
    State(secret_key): State<Arc<SecretKey>>,
    mut request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let init_data = request
        .headers()
        .get(HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or_else(|| {
            warn!(error = %AuthError::MissingHeader, "auth: rejected request");
            StatusCode::UNAUTHORIZED
        })?;

    let user_id =
        verify_init_data_inner(init_data, secret_key.expose(), Utc::now()).map_err(|error| {
            warn!(%error, "auth: rejected initData");
            StatusCode::UNAUTHORIZED
        })?;

    request.extensions_mut().insert(user_id);
    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BOT_TOKEN: &str = "1234567890:TESTTOKENforunittestsONLYxxxxxxxxxx";

    fn secret() -> SecretKey {
        derive_secret_key(BOT_TOKEN)
    }

    /// Build a signed initData query string matching the wire format Telegram
    /// produces. Pairs are signed in their unencoded form (per Telegram spec)
    /// and URL-encoded only when assembled into the query.
    fn signed(pairs: &[(&str, &str)]) -> String {
        let mut sorted: Vec<&(&str, &str)> = pairs.iter().collect();
        sorted.sort_by(|a, b| a.0.cmp(b.0));
        let data_check_string = sorted
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join("\n");

        let key = secret();
        let mut mac = HmacSha256::new_from_slice(key.expose()).expect("HMAC accepts any key size");
        mac.update(data_check_string.as_bytes());
        let hash = hex::encode(mac.finalize().into_bytes());

        let mut query: Vec<String> = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
            .collect();
        query.push(format!("hash={hash}"));
        query.join("&")
    }

    fn now_at(epoch_secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(epoch_secs, 0).unwrap()
    }

    fn verify(init: &str, now: DateTime<Utc>) -> Result<TelegramUserId, AuthError> {
        let key = secret();
        verify_init_data_inner(init, key.expose(), now)
    }

    #[test]
    fn happy_path_returns_user_id() {
        let init = signed(&[
            ("auth_date", "1700000000"),
            ("user", r#"{"id":42,"first_name":"Ada"}"#),
            ("query_id", "abc123"),
        ]);
        let uid = verify(&init, now_at(1_700_000_300)).unwrap();
        assert_eq!(uid, TelegramUserId(42));
    }

    #[test]
    fn missing_hash_field() {
        let init = "auth_date=1700000000&user=%7B%22id%22%3A1%7D";
        let err = verify(init, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::MissingField("hash")));
    }

    #[test]
    fn missing_user_field() {
        let init = signed(&[("auth_date", "1700000000")]);
        let err = verify(&init, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::MissingField("user")));
    }

    #[test]
    fn invalid_hash_rejected() {
        let init = signed(&[("auth_date", "1700000000"), ("user", r#"{"id":1}"#)]);
        let (head, last) = init.split_at(init.len() - 1);
        let flipped = if last == "0" { "1" } else { "0" };
        let bad = format!("{head}{flipped}");
        let err = verify(&bad, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::InvalidHash));
    }

    #[test]
    fn invalid_hash_encoding_rejected() {
        let init = signed(&[("auth_date", "1700000000"), ("user", r#"{"id":1}"#)]);
        let truncated = init
            .split('&')
            .filter(|p| !p.starts_with("hash="))
            .collect::<Vec<_>>()
            .join("&");
        let bad = format!("{truncated}&hash=zzzz");
        let err = verify(&bad, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::InvalidHashEncoding));
    }

    #[test]
    fn expired_token_rejected() {
        let init = signed(&[("auth_date", "1700000000"), ("user", r#"{"id":1}"#)]);
        let now = now_at(1_700_000_000 + 25 * 3600);
        let err = verify(&init, now).unwrap_err();
        assert!(matches!(err, AuthError::Expired));
    }

    #[test]
    fn future_skew_rejected() {
        let init = signed(&[("auth_date", "1700000300"), ("user", r#"{"id":1}"#)]);
        let now = now_at(1_700_000_300 - 120);
        let err = verify(&init, now).unwrap_err();
        assert!(matches!(err, AuthError::Future));
    }

    #[test]
    fn small_future_skew_accepted() {
        let init = signed(&[("auth_date", "1700000300"), ("user", r#"{"id":7}"#)]);
        let now = now_at(1_700_000_300 - 30);
        let uid = verify(&init, now).unwrap();
        assert_eq!(uid, TelegramUserId(7));
    }

    #[test]
    fn malformed_user_json_rejected() {
        let init = signed(&[("auth_date", "1700000000"), ("user", "not-json")]);
        let err = verify(&init, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::InvalidUser(_)));
    }

    #[test]
    fn invalid_auth_date_rejected() {
        let init = signed(&[("auth_date", "notanint"), ("user", r#"{"id":1}"#)]);
        let err = verify(&init, now_at(1_700_000_000)).unwrap_err();
        assert!(matches!(err, AuthError::InvalidAuthDate));
    }
}
