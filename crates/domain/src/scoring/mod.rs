//! Scoring rules and point calculation.
//!
//! This module is the single source of truth for "how points are awarded".
//! Everything here is pure (no I/O, no `async`) so it can be unit-tested
//! exhaustively without a database and reused from the events scorer, the
//! api preview endpoints, and any future tooling.
//!
//! Submodules:
//! - [`group_match`] — group-stage exact-score predictions.
//! - [`knockout_match`] — knockout-stage predictions with implicit ET/PK
//!   path encoding plus an explicit advancement-winner field.
//! - [`standings`] — group-position 1..4 + best-thirds set predictions.
//! - [`penalty`] — flat re-pick percentage applied to match scores only.
//!
//! See `/docs` and the plan file for the spec each submodule implements.

pub mod group_match;
pub mod knockout_match;
pub mod penalty;
pub mod standings;

#[cfg(test)]
mod tests;

use thiserror::Error;

pub use group_match::score_group_match;
pub use knockout_match::score_knockout_match;
pub use penalty::apply_repick_penalty;
pub use standings::{score_best_thirds, score_group_standings};

/// Errors a scoring function can return when its inputs are inconsistent.
///
/// Scoring is otherwise infallible: well-formed inputs always produce a
/// non-negative `i32`. These variants exist as a defense-in-depth check for
/// invariants the database CHECK constraints and `Prediction::is_consistent`
/// should already guarantee.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ScoringError {
    /// The match record is missing data required to score it (e.g. the
    /// regulation home/away score is `None` on a `Finished` match, or a
    /// knockout match marked as gone-to-pens has no `pens_winner_team_id`).
    #[error("match record is missing required outcome data: {0}")]
    MissingMatchData(&'static str),
    /// The prediction violates one of the cross-field invariants from
    /// `Prediction::is_consistent`.
    #[error("prediction is inconsistent: {0}")]
    InconsistentPrediction(&'static str),
    /// A knockout match prediction did not specify an advancement winner
    /// even though the regulation prediction was a draw.
    #[error("knockout draw prediction is missing advancement_winner_team_id")]
    MissingAdvancementWinner,
}

/// Convenience alias used by the scoring submodules.
pub type ScoringResult<T> = Result<T, ScoringError>;
