//! Postgres-backed `ScoringAuditRepository`.
//!
//! Owned by `services/events`. The scorer appends one row per successful
//! scoring run. Reads are not exposed yet — when an audit-driven UI lands,
//! a sibling read API can be added on top of the same table.

use async_trait::async_trait;
use domain::repository::{RepoResult, ScoringAuditRepository};
use domain::{AuditKind, ScoringAudit};
use sqlx::PgPool;

use super::mappers::classify_write_error;

/// Postgres-backed `ScoringAuditRepository`.
pub struct PgScoringAuditRepository {
    pub pool: PgPool,
}

impl PgScoringAuditRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScoringAuditRepository for PgScoringAuditRepository {
    async fn record(&self, audit: &ScoringAudit) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO scoring_audit (
                id, scored_at, audit_kind, prediction_id,
                pickem_group_id, user_id, points_awarded,
                scoring_rule_id, was_changed
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)
            "#,
            audit.id,
            audit.scored_at,
            kind_to_db(audit.audit_kind),
            audit.prediction_id,
            audit.pickem_group_id,
            audit.user_id.0,
            audit.points_awarded,
            audit.scoring_rule_id,
            audit.was_changed,
        )
        .execute(&self.pool)
        .await
        .map_err(classify_write_error)?;

        Ok(())
    }
}

fn kind_to_db(kind: AuditKind) -> &'static str {
    kind.as_str()
}
