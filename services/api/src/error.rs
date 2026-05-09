//! Error type returned by HTTP handlers + the corresponding `error-stack`
//! aliases for internal chaining.
//!
//! Why two flavors? Axum needs a concrete type that implements
//! `IntoResponse`, and the orphan rule prevents us from impl'ing
//! `IntoResponse` on `Report<ApiError>` (both come from external crates).
//! So handlers return `Result<T, ApiError>`, and any internal helper that
//! produces a chain-aware error returns `ApiResult<T>` (a
//! `Result<T, Report<ApiError>>`). The boundary between the two collapses
//! the report into an `ApiError::Internal` carrying the formatted chain.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use error_stack::Report;
use serde_json::json;
use thiserror::Error;

/// Top-level error returned from API handlers. Uses an explicit enum so the
/// HTTP status mapping lives in one place.
#[allow(dead_code)] // variants are constructed by route handlers (currently stubs)
#[derive(Debug, Error)]
pub enum ApiError {
    /// 401 — the request lacks a valid `X-Telegram-Init-Data` header.
    #[error("unauthorized")]
    Unauthorized,
    /// 403 — the caller is authenticated but not allowed to act on the
    /// addressed pickem (typically: not a member).
    #[error("forbidden")]
    Forbidden,
    /// 404 — the addressed resource does not exist.
    #[error("not found")]
    NotFound,
    /// 400 — the request payload failed validation.
    #[error("bad request: {0}")]
    BadRequest(String),
    /// 500 — anything else. Carries the formatted error chain for logging.
    #[error("internal error: {0}")]
    Internal(String),
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match &self {
            ApiError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            ApiError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            ApiError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            ApiError::BadRequest(_) => (StatusCode::BAD_REQUEST, "bad_request"),
            ApiError::Internal(_) => (StatusCode::INTERNAL_SERVER_ERROR, "internal_error"),
        };
        let body = Json(json!({
            "error": code,
            "message": self.to_string(),
        }));
        (status, body).into_response()
    }
}

/// Collapse any error-stack report into an `ApiError::Internal`, preserving
/// the full formatted chain in the message. Use this at the handler boundary
/// when bubbling up internal errors.
impl<C> From<Report<C>> for ApiError
where
    C: std::error::Error + Send + Sync + 'static,
{
    fn from(report: Report<C>) -> Self {
        ApiError::Internal(format!("{report:?}"))
    }
}

/// Convenience alias for a fully-formed report of an API error, for internal
/// use (helpers, repositories called from handlers).
#[allow(dead_code)] // consumed by handler-internal helpers (currently stubs)
pub type ApiReport = Report<ApiError>;

/// Result type for internal helpers that want to chain context with
/// `error-stack`. Handlers themselves still return `Result<T, ApiError>`
/// because Axum's `IntoResponse` cannot be implemented for `Report<ApiError>`
/// (orphan rule).
#[allow(dead_code)] // consumed by handler-internal helpers (currently stubs)
pub type ApiResult<T> = Result<T, ApiReport>;
