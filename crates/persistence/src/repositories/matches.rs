use async_trait::async_trait;
use domain::repository::{MatchRepository, RepoResult};
use domain::Match;
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `MatchRepository`.
pub struct PgMatchRepository {
    pub pool: PgPool,
}

impl PgMatchRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl MatchRepository for PgMatchRepository {
    async fn upsert(&self, _match_: &Match) -> RepoResult<()> {
        // TODO: ON CONFLICT (round_id, external_id) DO UPDATE SET status, scores ...
        todo!("PgMatchRepository::upsert")
    }

    async fn list_by_round(&self, _round_id: Uuid) -> RepoResult<Vec<Match>> {
        todo!("PgMatchRepository::list_by_round")
    }

    async fn find(&self, _id: Uuid) -> RepoResult<Option<Match>> {
        todo!("PgMatchRepository::find")
    }
}
