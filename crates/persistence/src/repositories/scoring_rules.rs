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

const SELECT_COLUMNS: &str = r#"
    id, name,
    gs_miss_points, gs_one_team_points, gs_sign_only_points,
    gs_sign_plus_one_points, gs_exact_points,
    ko_adv_points, ko_et_flag_points, ko_pk_flag_points, ko_combo_points,
    st_pos1_points, st_pos2_points, st_pos3_points, st_pos4_points,
    st_full_combo_points, st_best_third_points,
    repick_penalty_pct
"#;

#[async_trait]
impl ScoringRuleRepository for PgScoringRuleRepository {
    async fn find(&self, id: Uuid) -> RepoResult<Option<ScoringRule>> {
        let query = format!("SELECT {SELECT_COLUMNS} FROM scoring_rules WHERE id = $1");
        let row = sqlx::query(&query)
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
        let query =
            format!("SELECT {SELECT_COLUMNS} FROM scoring_rules WHERE name = 'default' LIMIT 1");
        let row = sqlx::query(&query)
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
        gs_miss_points: row.get("gs_miss_points"),
        gs_one_team_points: row.get("gs_one_team_points"),
        gs_sign_only_points: row.get("gs_sign_only_points"),
        gs_sign_plus_one_points: row.get("gs_sign_plus_one_points"),
        gs_exact_points: row.get("gs_exact_points"),
        ko_adv_points: row.get("ko_adv_points"),
        ko_et_flag_points: row.get("ko_et_flag_points"),
        ko_pk_flag_points: row.get("ko_pk_flag_points"),
        ko_combo_points: row.get("ko_combo_points"),
        st_pos1_points: row.get("st_pos1_points"),
        st_pos2_points: row.get("st_pos2_points"),
        st_pos3_points: row.get("st_pos3_points"),
        st_pos4_points: row.get("st_pos4_points"),
        st_full_combo_points: row.get("st_full_combo_points"),
        st_best_third_points: row.get("st_best_third_points"),
        repick_penalty_pct: row.get("repick_penalty_pct"),
    }
}
