use async_trait::async_trait;
use domain::repository::{RepoResult, ScoringRuleRepository};
use domain::ScoringRule;
use sqlx::PgPool;
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
    async fn find(&self, _id: Uuid) -> RepoResult<Option<ScoringRule>> {
        todo!("PgScoringRuleRepository::find")
    }

    async fn default_rule(&self) -> RepoResult<ScoringRule> {
        // The migration seeds a rule named 'default'.
        todo!("PgScoringRuleRepository::default_rule")
    }
}
