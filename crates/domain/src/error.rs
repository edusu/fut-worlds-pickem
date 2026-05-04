//! Error types for the domain layer.
//!
//! `RepositoryError` is the high-level taxonomy any persistence adapter is
//! allowed to surface. Implementations wrap upstream errors (sqlx, network,
//! etc.) by adding them to the `Report` chain via `change_context` and
//! `attach` / `attach_with` rather than embedding them in the variant payload.
//!
//! Use `RepositoryReport` when storing a report independently and
//! `RepoResult<T>` as the return type for repository methods.

use error_stack::Report;
use thiserror::Error;

/// High-level taxonomy of repository failures.
///
/// Variants are intentionally light on payload data — context and source
/// errors are attached to the surrounding `Report` chain.
#[derive(Debug, Error)]
pub enum RepositoryError {
    /// The requested entity does not exist.
    #[error("entity not found")]
    NotFound,
    /// A database integrity constraint was violated (FK, unique, NOT NULL).
    #[error("integrity violation")]
    Integrity,
    /// The underlying storage backend (Postgres, network) failed.
    #[error("backend error")]
    Backend,
}

/// Convenience alias for a fully-formed report of a repository error.
pub type RepositoryReport = Report<RepositoryError>;

/// Result type for every repository method.
pub type RepoResult<T> = Result<T, RepositoryReport>;
