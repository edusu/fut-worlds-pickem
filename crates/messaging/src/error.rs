//! Error types for the messaging crate.
//!
//! Source errors (NATS, serde_json) are not embedded in the variants —
//! callers chain them with `error_stack::ResultExt::change_context` so the
//! resulting `Report` carries the full causal chain plus any
//! `attach` / `attach_with` context (topic, payload size, etc.).

use error_stack::Report;
use thiserror::Error;

/// High-level taxonomy of messaging failures.
#[derive(Debug, Error)]
pub enum MessagingError {
    /// A NATS publish or subscribe operation failed.
    #[error("NATS operation failed")]
    Nats,
    /// JSON serialization or deserialization of an event failed.
    #[error("event (de)serialization failed")]
    Serde,
}

/// Convenience alias for a fully-formed report of a messaging error.
pub type MessagingReport = Report<MessagingError>;

/// Result type for every public function in this crate.
pub type MessagingResult<T> = Result<T, MessagingReport>;
