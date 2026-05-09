//! Cross-repository mapping helpers.
//!
//! Holds the small set of conversions more than one repository needs —
//! mainly the `sqlx::Error` → `RepositoryError` classifier shared across
//! upserts and the `open`/`closed`/`scored` lifecycle enum mapping that
//! `tournament_groups` and `knockout_phases` both store as plain TEXT
//! constrained by the same CHECK.

use domain::{KnockoutPhaseState, RepositoryError, TournamentGroupState};
use error_stack::Report;

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

/// Common shape of the `open`/`closed`/`scored` lifecycle enum stored as
/// plain TEXT on `tournament_groups.state` and `knockout_phases.state`.
/// The two domain enums (`TournamentGroupState`, `KnockoutPhaseState`)
/// share the same set of values but stay distinct types so a caller
/// can't accidentally pass a phase state where a group state is expected.
pub(crate) trait LifecycleState: Sized + Copy {
    fn as_db_str(self) -> &'static str;
    fn from_db_str(s: &str) -> Option<Self>;
    fn type_name() -> &'static str;
}

impl LifecycleState for TournamentGroupState {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Scored => "scored",
        }
    }
    fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "closed" => Self::Closed,
            "scored" => Self::Scored,
            _ => return None,
        })
    }
    fn type_name() -> &'static str {
        "tournament_group_state"
    }
}

impl LifecycleState for KnockoutPhaseState {
    fn as_db_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Closed => "closed",
            Self::Scored => "scored",
        }
    }
    fn from_db_str(s: &str) -> Option<Self> {
        Some(match s {
            "open" => Self::Open,
            "closed" => Self::Closed,
            "scored" => Self::Scored,
            _ => return None,
        })
    }
    fn type_name() -> &'static str {
        "knockout_phase_state"
    }
}

/// Parse a `state` column value back into the typed enum, surfacing an
/// unrecognised string as `Backend` (a schema↔enum drift, our bug).
pub(crate) fn parse_state<S: LifecycleState>(s: &str) -> Result<S, Report<RepositoryError>> {
    S::from_db_str(s).ok_or_else(|| {
        Report::new(RepositoryError::Backend)
            .attach(format!("unknown {} in DB: {s}", S::type_name()))
    })
}
