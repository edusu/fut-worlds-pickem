//! Cross-repository mapping helpers.
//!
//! Holds the small set of conversions that more than one repository needs —
//! mainly enum ↔ string serialization for columns stored as plain TEXT and
//! the `sqlx::Error` → `RepositoryError` classifier shared across upserts.

use domain::{Phase, RepositoryError};
use error_stack::Report;

/// Canonical strings for `Phase`. Mirrors the `serde(rename_all="lowercase")`
/// on the enum and the values seeded in migration `0002`.
pub(crate) fn phase_to_str(phase: Phase) -> &'static str {
    match phase {
        Phase::Group => "group",
        Phase::Knockout => "knockout",
    }
}

/// Parse a `phase` column value back into the domain enum. An unrecognised
/// string indicates schema↔enum drift (a bug in our code), so we surface it
/// as `Backend` rather than `Integrity`.
pub(crate) fn phase_from_str(s: &str) -> Result<Phase, Report<RepositoryError>> {
    match s {
        "group" => Ok(Phase::Group),
        "knockout" => Ok(Phase::Knockout),
        other => {
            Err(Report::new(RepositoryError::Backend)
                .attach(format!("unknown phase in DB: {other}")))
        }
    }
}

/// Classify an `sqlx::Error` from a write into the appropriate
/// `RepositoryError` bucket. FK / unique / CHECK violations become
/// `Integrity`; everything else becomes `Backend`.
pub(crate) fn classify_write_error(e: sqlx::Error) -> Report<RepositoryError> {
    let kind = match &e {
        sqlx::Error::Database(db)
            if db.is_foreign_key_violation()
                || db.is_unique_violation()
                || db.is_check_violation() =>
        {
            RepositoryError::Integrity
        }
        _ => RepositoryError::Backend,
    };
    Report::new(e).change_context(kind)
}
