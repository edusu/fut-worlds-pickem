use async_trait::async_trait;
use domain::repository::{RepoResult, UserRepository};
use domain::{TelegramUserId, User};
use sqlx::PgPool;

/// Postgres-backed `UserRepository`.
pub struct PgUserRepository {
    pub pool: PgPool,
}

impl PgUserRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl UserRepository for PgUserRepository {
    async fn upsert(&self, _user: &User) -> RepoResult<()> {
        // TODO: INSERT INTO users (...) ON CONFLICT (telegram_id) DO UPDATE ...
        todo!("PgUserRepository::upsert")
    }

    async fn find(&self, _id: TelegramUserId) -> RepoResult<Option<User>> {
        // TODO: SELECT ... FROM users WHERE telegram_id = $1
        todo!("PgUserRepository::find")
    }
}
