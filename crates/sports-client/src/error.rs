//! Error types for the football-data sports client.
//!
//! Source errors (reqwest, serde) are not embedded — they are attached to the
//! report chain via `change_context` so callers can inspect the full causal
//! chain plus any printable context (URL, status code, request id).

use error_stack::Report;
use thiserror::Error;

/// High-level taxonomy of failures from the upstream sports provider.
#[derive(Debug, Error)]
pub enum SportsClientError {
    /// HTTP transport-level failure (DNS, TCP, TLS, timeout).
    #[error("HTTP transport error")]
    Http,
    /// Upstream returned a non-2xx status code.
    #[error("upstream returned a non-success status")]
    Upstream,
    /// Response body could not be deserialized to the expected DTO.
    #[error("response decode error")]
    Decode,
}

/// Convenience alias for a fully-formed report of a sports-client error.
pub type SportsClientReport = Report<SportsClientError>;

/// Result type for every sports-client operation.
pub type SportsResult<T> = Result<T, SportsClientReport>;
