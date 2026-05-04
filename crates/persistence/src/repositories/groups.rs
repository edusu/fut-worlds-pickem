use async_trait::async_trait;
use domain::repository::{GroupRepository, RepoResult};
use domain::{Group, GroupMember, TelegramChatId};
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
    async fn create(&self, _group: &Group) -> RepoResult<()> {
        todo!("PgGroupRepository::create")
    }

    async fn find_by_chat(&self, _chat_id: TelegramChatId) -> RepoResult<Option<Group>> {
        todo!("PgGroupRepository::find_by_chat")
    }

    async fn add_member(&self, _member: &GroupMember) -> RepoResult<()> {
        todo!("PgGroupRepository::add_member")
    }

    async fn list_members(&self, _group_id: Uuid) -> RepoResult<Vec<GroupMember>> {
        todo!("PgGroupRepository::list_members")
    }
}
