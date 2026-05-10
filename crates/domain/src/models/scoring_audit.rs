use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::TelegramUserId;

/// Discriminator for `scoring_audit.audit_kind`. Identifies which
/// prediction table the `prediction_id` refers to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuditKind {
    /// Row in `predictions`.
    Match,
    /// Row in `group_standings_predictions`.
    GroupStandings,
    /// Row in `best_thirds_predictions`.
    BestThirds,
}

impl AuditKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AuditKind::Match => "match",
            AuditKind::GroupStandings => "group_standings",
            AuditKind::BestThirds => "best_thirds",
        }
    }
}

/// Audit row appended after every successful scoring run. The whole
/// struct is read-only as far as callers are concerned: the scorer
/// builds it, persists it via the repository, and never edits it again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoringAudit {
    pub id: Uuid,
    pub scored_at: DateTime<Utc>,
    pub audit_kind: AuditKind,
    pub prediction_id: Uuid,
    pub pickem_group_id: Uuid,
    pub user_id: TelegramUserId,
    pub points_awarded: i32,
    pub scoring_rule_id: Uuid,
    pub was_changed: bool,
}
