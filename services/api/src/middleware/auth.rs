//! Mini App `initData` validation middleware.
//!
//! Telegram delivers a signed query string to the Mini App; we forward it to
//! the API as the `X-Telegram-Init-Data` header. The signature is HMAC-SHA256
//! using a secret derived from the bot token (see Telegram docs:
//! <https://core.telegram.org/bots/webapps#validating-data-received-via-the-mini-app>).
//!
//! INSECURE STUB: the current body only checks the header is present. Before
//! shipping, replace the TODO block with full HMAC validation against the bot
//! token and an `auth_date` freshness check. Do not deploy as-is.

use axum::extract::Request;
use axum::http::StatusCode;
use axum::middleware::Next;
use axum::response::Response;

const HEADER: &str = "X-Telegram-Init-Data";

/// Verify the `X-Telegram-Init-Data` header. Failure returns 401.
pub async fn verify_init_data(request: Request, next: Next) -> Result<Response, StatusCode> {
    let _init_data = request
        .headers()
        .get(HEADER)
        .and_then(|h| h.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    // TODO: HMAC-SHA256 over the sorted key=value pairs (excluding `hash`),
    // using `HMAC(SHA256("WebAppData"), bot_token)` as the secret. Compare in
    // constant time. Reject if `auth_date` is older than ~24h. Until then,
    // this middleware is effectively a no-op gate — DO NOT SHIP.
    Ok(next.run(request).await)
}
