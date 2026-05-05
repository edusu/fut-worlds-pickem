use async_trait::async_trait;
use domain::repository::{PredictionRepository, RepoResult};
use domain::{Prediction, TelegramUserId};
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `PredictionRepository`.
pub struct PgPredictionRepository {
    pub pool: PgPool,
}

impl PgPredictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl PredictionRepository for PgPredictionRepository {
    async fn upsert(&self, _prediction: &Prediction) -> RepoResult<()> {
        // TODO: ON CONFLICT (user_id, match_id) DO UPDATE — predictions can
        // be revised until the round deadline.
        todo!("PgPredictionRepository::upsert")
    }

    async fn list_by_match(&self, _match_id: Uuid) -> RepoResult<Vec<Prediction>> {
        todo!("PgPredictionRepository::list_by_match")
    }

    async fn list_by_user(&self, _user_id: TelegramUserId) -> RepoResult<Vec<Prediction>> {
        todo!("PgPredictionRepository::list_by_user")
    }

    async fn list_by_pickem(&self, _pickem_group_id: Uuid) -> RepoResult<Vec<Prediction>> {
        todo!("PgPredictionRepository::list_by_pickem")
    }

    async fn record_points(&self, _prediction_id: Uuid, _points: i32) -> RepoResult<()> {
        todo!("PgPredictionRepository::record_points")
    }

    async fn mark_as_changed(&self, _prediction_id: Uuid) -> RepoResult<()> {
        todo!("PgPredictionRepository::mark_as_changed")
    }
}
