use async_trait::async_trait;
use domain::repository::{GroupRepository, RepoResult};
use domain::{Group, GroupMember, RepositoryError, TelegramChatId, TelegramUserId};
use error_stack::{Report, ResultExt};
use sqlx::{PgPool, Row};
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

        sqlx::query(
            r#"
            INSERT INTO groups (id, telegram_chat_id, name, owner_id, scoring_rule_id, created_at)
            VALUES ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(group.id)
        .bind(group.telegram_chat_id.0)
        .bind(&group.name)
        .bind(group.owner_id.0)
        .bind(group.scoring_rule_id)
        .bind(group.created_at)
        .execute(&mut *tx)
        .await
        .map_err(|e| {
            let kind = match &e {
                sqlx::Error::Database(db) if db.is_unique_violation() => RepositoryError::Integrity,
                _ => RepositoryError::Backend,
            };
            Report::new(e).change_context(kind)
        })?;

        sqlx::query(
            r#"
            INSERT INTO group_members (group_id, user_id, joined_at)
            VALUES ($1, $2, $3)
            "#,
        )
        .bind(member.group_id)
        .bind(member.user_id.0)
        .bind(member.joined_at)
        .execute(&mut *tx)
        .await
        .change_context(RepositoryError::Backend)?;

        tx.commit().await.change_context(RepositoryError::Backend)?;

        Ok(())
    }

    /// Locate the pickem bound to a Telegram chat. `None` means no pickem
    /// has been created there yet.
    async fn find_by_chat(&self, chat_id: TelegramChatId) -> RepoResult<Option<Group>> {
        let row = sqlx::query(
            r#"
            SELECT id, telegram_chat_id, name, owner_id, scoring_rule_id, created_at
            FROM groups
            WHERE telegram_chat_id = $1
            "#,
        )
        .bind(chat_id.0)
        .fetch_optional(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(row.map(|row| Group {
            id: row.get("id"),
            telegram_chat_id: TelegramChatId(row.get("telegram_chat_id")),
            name: row.get("name"),
            owner_id: TelegramUserId(row.get("owner_id")),
            scoring_rule_id: row.get("scoring_rule_id"),
            created_at: row.get("created_at"),
        }))
    }

    /// Idempotently add a user to a pickem. Re-running for an existing
    /// `(group_id, user_id)` is a silent no-op thanks to the composite PK
    /// and `ON CONFLICT DO NOTHING`.
    async fn add_member(&self, member: &GroupMember) -> RepoResult<()> {
        sqlx::query(
            r#"
            INSERT INTO group_members (group_id, user_id, joined_at)
            VALUES ($1, $2, $3)
            ON CONFLICT (group_id, user_id) DO NOTHING
            "#,
        )
        .bind(member.group_id)
        .bind(member.user_id.0)
        .bind(member.joined_at)
        .execute(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(())
    }

    /// List the members of a pickem in join order — used by `/ranking` to
    /// know who participates before pulling per-user point totals.
    async fn list_members(&self, group_id: Uuid) -> RepoResult<Vec<GroupMember>> {
        let rows = sqlx::query(
            r#"
            SELECT group_id, user_id, joined_at
            FROM group_members
            WHERE group_id = $1
            ORDER BY joined_at ASC
            "#,
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .change_context(RepositoryError::Backend)?;

        Ok(rows
            .into_iter()
            .map(|row| GroupMember {
                group_id: row.get("group_id"),
                user_id: TelegramUserId(row.get("user_id")),
                joined_at: row.get("joined_at"),
            })
            .collect())
    }
}
