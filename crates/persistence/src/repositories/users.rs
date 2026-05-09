use async_trait::async_trait;
use domain::repository::{RepoResult, UserRepository};
use domain::{RepositoryError, TelegramUserId, User};
use error_stack::ResultExt;
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
    /// Upsert a Telegram user. `created_at` is set on insert and preserved on
    /// update — only the mutable profile fields are refreshed, matching
    /// Telegram's contract (the user can change name or language, but the
    /// registration moment is fixed).
    async fn upsert(&self, user: &User) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO users (telegram_id, username, first_name, language_code)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (telegram_id) DO UPDATE SET
                username = EXCLUDED.username,
                first_name = EXCLUDED.first_name,
                language_code = EXCLUDED.language_code
            "#,
            user.telegram_id.0,
            user.username.as_deref(),
            user.first_name,
            user.language_code.as_deref(),
        )
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(())
    }

    /// Look up a user by Telegram id. `None` means the user has never
    /// interacted with the bot.
    async fn find(&self, id: TelegramUserId) -> RepoResult<Option<User>> {
        let row = sqlx::query!(
            r#"
            SELECT telegram_id, username, first_name, language_code, created_at
            FROM users
            WHERE telegram_id = $1
            "#,
            id.0,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|r| User {
            telegram_id: TelegramUserId(r.telegram_id),
            username: r.username,
            first_name: r.first_name,
            language_code: r.language_code,
            created_at: r.created_at,
        }))
    }
}
