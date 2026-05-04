use async_trait::async_trait;
use chrono::{DateTime, Utc};
use domain::repository::{RepoResult, RoundRepository};
use domain::{Round, RoundState};
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `RoundRepository`.
pub struct PgRoundRepository {
    pub pool: PgPool,
}

impl PgRoundRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl RoundRepository for PgRoundRepository {
    async fn create(&self, _round: &Round) -> RepoResult<()> {
        todo!("PgRoundRepository::create")
    }

    async fn list_active(&self, _group_id: Uuid) -> RepoResult<Vec<Round>> {
        todo!("PgRoundRepository::list_active")
    }

    async fn set_state(&self, _round_id: Uuid, _state: RoundState) -> RepoResult<()> {
        todo!("PgRoundRepository::set_state")
    }

    async fn list_due_for_close(&self, _before: DateTime<Utc>) -> RepoResult<Vec<Round>> {
        todo!("PgRoundRepository::list_due_for_close")
    }
}
