//! Cross-cutting error type. Service-specific errors should wrap reports of
//! `SharedError` rather than re-deriving identical variants.

use error_stack::Report;
use thiserror::Error;

/// High-level taxonomy of shared utility failures.
///
/// Specific keys, paths, or values are attached via
/// `error_stack::ResultExt::attach_with` instead of being embedded
/// in the variants.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum SharedError {
    /// A required configuration value was missing or empty.
    #[error("missing required config value")]
    MissingConfig,
}

/// Convenience alias for a fully-formed report of a shared error.
pub type SharedReport = Report<SharedError>;

/// Result type for every public function in this crate.
pub type SharedResult<T> = Result<T, SharedReport>;

/// Collapse any error-stack report into an `anyhow::Error` carrying the full
/// formatted chain. Useful in `main` functions that return `anyhow::Result<()>`
/// — error-stack's `Report` deliberately does not implement `std::error::Error`,
/// so the standard `?` operator cannot bridge the two type universes on its
/// own. Use as `.map_err(shared::report_to_anyhow)`.
pub fn report_to_anyhow<C>(report: Report<C>) -> anyhow::Error {
    anyhow::anyhow!("{report:?}")
}
