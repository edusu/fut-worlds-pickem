//! Repository traits — abstract persistence ports.
//!
//! Each service depends on these traits, never on a concrete database driver.
//! Concrete implementations live in the `persistence` crate.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;
use uuid::Uuid;

use crate::{
    Group, GroupMember, Match, Prediction, Round, RoundState, ScoringRule, TelegramChatId,
    TelegramUserId, User,
};

/// Errors any repository implementation may surface.
#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("entity not found")]
    NotFound,
    #[error("integrity violation: {0}")]
    Integrity(String),
    #[error("backend error: {0}")]
    Backend(#[source] Box<dyn std::error::Error + Send + Sync>),
}

pub type RepoResult<T> = Result<T, RepositoryError>;

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn upsert(&self, user: &User) -> RepoResult<()>;
    async fn find(&self, id: TelegramUserId) -> RepoResult<Option<User>>;
}

#[async_trait]
pub trait GroupRepository: Send + Sync {
    async fn create(&self, group: &Group) -> RepoResult<()>;
    async fn find_by_chat(&self, chat_id: TelegramChatId) -> RepoResult<Option<Group>>;
    async fn add_member(&self, member: &GroupMember) -> RepoResult<()>;
    async fn list_members(&self, group_id: Uuid) -> RepoResult<Vec<GroupMember>>;
}

#[async_trait]
pub trait RoundRepository: Send + Sync {
    async fn create(&self, round: &Round) -> RepoResult<()>;
    async fn list_active(&self, group_id: Uuid) -> RepoResult<Vec<Round>>;
    async fn set_state(&self, round_id: Uuid, state: RoundState) -> RepoResult<()>;
    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<Round>>;
}

#[async_trait]
pub trait MatchRepository: Send + Sync {
    async fn upsert(&self, match_: &Match) -> RepoResult<()>;
    async fn list_by_round(&self, round_id: Uuid) -> RepoResult<Vec<Match>>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<Match>>;
}

#[async_trait]
pub trait PredictionRepository: Send + Sync {
    async fn upsert(&self, prediction: &Prediction) -> RepoResult<()>;
    async fn list_by_match(&self, match_id: Uuid) -> RepoResult<Vec<Prediction>>;
    async fn list_by_user(&self, user_id: TelegramUserId) -> RepoResult<Vec<Prediction>>;
    async fn record_points(&self, prediction_id: Uuid, points: i32) -> RepoResult<()>;
}

#[async_trait]
pub trait ScoringRuleRepository: Send + Sync {
    async fn find(&self, id: Uuid) -> RepoResult<Option<ScoringRule>>;
    async fn default_rule(&self) -> RepoResult<ScoringRule>;
}
