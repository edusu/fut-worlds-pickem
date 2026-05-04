use async_trait::async_trait;
use domain::repository::{RepoResult, ScoringRuleRepository};
use domain::{RepositoryError, ScoringRule};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
use uuid::Uuid;

/// Postgres-backed `ScoringRuleRepository`.
pub struct PgScoringRuleRepository {
    pub pool: PgPool,
}

impl PgScoringRuleRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl ScoringRuleRepository for PgScoringRuleRepository {
    async fn find(&self, id: Uuid) -> RepoResult<Option<ScoringRule>> {
        let row = sqlx::query(
            r#"
            SELECT id, name, exact_score_points, correct_diff_points,
                   correct_sign_points, miss_points
            FROM scoring_rules
            WHERE id = $1
            "#,
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(row_to_rule))
    }

    /// Resolve the scoring rule seeded as `'default'` by migration 0004.
    /// Every new pickem starts pointing to this row until its owner
    /// customizes it. Absence is treated as `RepositoryError::NotFound` —
    /// in practice it would only happen if migrations were not applied.
    async fn default_rule(&self) -> RepoResult<ScoringRule> {
        let row = sqlx::query(
            r#"
            SELECT id, name, exact_score_points, correct_diff_points,
                   correct_sign_points, miss_points
            FROM scoring_rules
            WHERE name = 'default'
            LIMIT 1
            "#,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        row.map(row_to_rule)
            .ok_or_else(|| Report::new(RepositoryError::NotFound))
    }
}

fn row_to_rule(row: sqlx::postgres::PgRow) -> ScoringRule {
    ScoringRule {
        id: row.get("id"),
        name: row.get("name"),
        exact_score_points: row.get("exact_score_points"),
        correct_diff_points: row.get("correct_diff_points"),
        correct_sign_points: row.get("correct_sign_points"),
        miss_points: row.get("miss_points"),
    }
}
