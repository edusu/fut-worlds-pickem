use async_trait::async_trait;
use domain::repository::{PredictionRepository, RepoResult};
use domain::{Prediction, RepositoryError, TelegramUserId};
use error_stack::Report;
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `PredictionRepository`.
///
/// Currently a stub. Every method returns `RepositoryError::Backend` with a
/// "not implemented" attachment so the api degrades gracefully (a 500 with a
/// clear log message) instead of panicking. Real implementations land with
/// the predictions write path; until then keep this surface non-fatal so the
/// rest of the service can boot and serve read-only flows.
pub struct PgPredictionRepository {
    pub pool: PgPool,
}

impl PgPredictionRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

fn unimplemented(method: &'static str) -> Report<RepositoryError> {
    Report::new(RepositoryError::Backend).attach(format!(
        "PgPredictionRepository::{method} is not implemented yet"
    ))
}

#[async_trait]
impl PredictionRepository for PgPredictionRepository {
    async fn upsert(&self, _prediction: &Prediction) -> RepoResult<()> {
        Err(unimplemented("upsert"))
    }

    async fn list_by_match(&self, _match_id: Uuid) -> RepoResult<Vec<Prediction>> {
        Err(unimplemented("list_by_match"))
    }

    async fn list_by_user(&self, _user_id: TelegramUserId) -> RepoResult<Vec<Prediction>> {
        Err(unimplemented("list_by_user"))
    }

    async fn list_by_pickem(&self, _pickem_group_id: Uuid) -> RepoResult<Vec<Prediction>> {
        Err(unimplemented("list_by_pickem"))
    }

    async fn record_points(&self, _prediction_id: Uuid, _points: i32) -> RepoResult<()> {
        Err(unimplemented("record_points"))
    }

    async fn mark_as_changed(&self, _prediction_id: Uuid) -> RepoResult<()> {
        Err(unimplemented("mark_as_changed"))
    }
}
