use async_trait::async_trait;
use domain::repository::{GroupRepository, RepoResult};
use domain::{Group, GroupMember, RepositoryError, TelegramChatId, TelegramUserId};
use error_stack::{Report, ResultExt};
use sqlx::PgPool;
use uuid::Uuid;

/// Postgres-backed `GroupRepository`.
pub struct PgGroupRepository {
    pub pool: PgPool,
}

impl PgGroupRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl GroupRepository for PgGroupRepository {
    /// Atomically insert the pickem row and the owner's membership row.
    /// Both statements run on the same `Transaction`, so a failure of the
    /// second rolls back the first. Surfaces `RepositoryError::Integrity`
    /// when the chat already has a pickem (UNIQUE on `telegram_chat_id`),
    /// which the caller turns into "you already have a pickem here".
    async fn create_with_owner(&self, group: &Group, member: &GroupMember) -> RepoResult<()> {
        let mut tx = self
            .pool
            .begin()
            .await
            .change_context(RepositoryError::Backend)?;

        sqlx::query!(
            r#"
            INSERT INTO groups (id, telegram_chat_id, name, owner_id, scoring_rule_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
            group.id,
            group.telegram_chat_id.0,
            group.name,
            group.owner_id.0,
            group.scoring_rule_id,
            group.created_at,
        )
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            let kind = match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => RepositoryError::Integrity,
                _ => RepositoryError::Backend,
            };
            Report::new(e).change_context(kind)
        })?;

        sqlx::query!(
            r#"
            INSERT INTO group_members (group_id, user_id, joined_at)
            VALUES ($1, $2, $3)
            "#,
            member.group_id,
            member.user_id.0,
            member.joined_at,
        )
        .execute(&mut *tx)
        .await
        .change_context(RepositoryError::Backend)?;

        tx.commit().await.change_context(RepositoryError::Backend)?;

        Ok(())
    }

    /// Locate the pickem bound to a Telegram chat. `None` means no pickem
    /// has been created there yet.
    async fn find_by_chat(&self, chat_id: TelegramChatId) -> RepoResult<Option<Group>> {
        let row = sqlx::query!(
            r#"
            SELECT id, telegram_chat_id, name, owner_id, scoring_rule_id, created_at
            FROM groups
            WHERE telegram_chat_id = $1
            "#,
            chat_id.0,
        )
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|r| Group {
            id: r.id,
            telegram_chat_id: TelegramChatId(r.telegram_chat_id),
            name: r.name,
            owner_id: TelegramUserId(r.owner_id),
            scoring_rule_id: r.scoring_rule_id,
            created_at: r.created_at,
        }))
    }

    /// Idempotently add a user to a pickem. Re-running for an existing
    /// `(group_id, user_id)` is a silent no-op thanks to the composite PK
    /// and `ON CONFLICT DO NOTHING`.
    async fn add_member(&self, member: &GroupMember) -> RepoResult<()> {
        sqlx::query!(
            r#"
            INSERT INTO group_members (group_id, user_id, joined_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (group_id, user_id) DO NOTHING
            "#,
            member.group_id,
            member.user_id.0,
            member.joined_at,
        )
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(())
    }

    /// Whether the user is a member of the pickem. Single `EXISTS` so the
    /// query stays O(1) regardless of roster size.
    async fn is_member(&self, group_id: Uuid, user_id: TelegramUserId) -> RepoResult<bool> {
        let row = sqlx::query!(
            r#"
            SELECT EXISTS (
                SELECT 1 FROM group_members
                WHERE group_id = $1 AND user_id = $2
            ) AS "exists!"
            "#,
            group_id,
            user_id.0,
        )
        .fetch_one(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.exists)
    }

    /// List the members of a pickem in join order — used by `/ranking` to
    /// know who participates before pulling per-user point totals.
    async fn list_members(&self, group_id: Uuid) -> RepoResult<Vec<GroupMember>> {
        let rows = sqlx::query!(
            r#"
            SELECT group_id, user_id, joined_at
            FROM group_members
            WHERE group_id = $1
            ORDER BY joined_at ASC
            "#,
            group_id,
        )
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|r| GroupMember {
                group_id: r.group_id,
                user_id: TelegramUserId(r.user_id),
                joined_at: r.joined_at,
            })
            .collect())
    }
}
