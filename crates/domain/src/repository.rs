//! Repository traits — abstract persistence ports.
//!
//! Each service depends on these traits, never on a concrete database driver.
//! Concrete implementations live in the `persistence` crate. The error
//! taxonomy lives in `crate::error`.

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use uuid::Uuid;

pub use crate::error::RepoResult;

use crate::{
    BestThirdsPrediction, Group, GroupMember, GroupStandingsPrediction, Match, Prediction, Round,
    RoundState, ScoringRule, Team, TelegramChatId, TelegramUserId, TournamentGroup,
    TournamentGroupAssignment, User,
};

#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn upsert(&self, user: &User) -> RepoResult<()>;
    async fn find(&self, id: TelegramUserId) -> RepoResult<Option<User>>;
}

#[async_trait]
pub trait GroupRepository: Send + Sync {
    /// Look up the pickem bound to a Telegram chat. `None` means there is
    /// no pickem in that chat yet.
    async fn find_by_chat(&self, chat_id: TelegramChatId) -> RepoResult<Option<Group>>;

    /// Atomically create a pickem and register the invoking user as its
    /// first member. Both writes happen in a single transaction — there is
    /// no API to create a `Group` without seeding `group_members`, because
    /// a pickem with zero members is an invalid state for the bot to ever
    /// observe.
    async fn create_with_owner(&self, group: &Group, member: &GroupMember) -> RepoResult<()>;

    /// Idempotently append a member to an existing pickem. Re-adding the
    /// same `(group_id, user_id)` pair is a silent no-op.
    async fn add_member(&self, member: &GroupMember) -> RepoResult<()>;

    /// Members of a pickem in join order.
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
    /// Insert or update a prediction. On UPDATE (i.e. an edit on an existing
    /// (user, match) pair) the implementation must set `was_changed = true`
    /// so the scorer applies the re-pick penalty. The first insert leaves
    /// `was_changed = false`.
    async fn upsert(&self, prediction: &Prediction) -> RepoResult<()>;
    async fn list_by_match(&self, match_id: Uuid) -> RepoResult<Vec<Prediction>>;
    async fn list_by_user(&self, user_id: TelegramUserId) -> RepoResult<Vec<Prediction>>;
    /// All predictions belonging to users in a given pickem (joining via the
    /// rounds → matches → predictions chain). Used by the ranking aggregator.
    async fn list_by_pickem(&self, pickem_group_id: Uuid) -> RepoResult<Vec<Prediction>>;
    async fn record_points(&self, prediction_id: Uuid, points: i32) -> RepoResult<()>;
    /// Defensive setter — `upsert` already flips this on update, but a
    /// dedicated method keeps the api endpoint that handles edits explicit
    /// about its intent.
    async fn mark_as_changed(&self, prediction_id: Uuid) -> RepoResult<()>;
}

#[async_trait]
pub trait ScoringRuleRepository: Send + Sync {
    async fn find(&self, id: Uuid) -> RepoResult<Option<ScoringRule>>;
    async fn default_rule(&self) -> RepoResult<ScoringRule>;
}

#[async_trait]
pub trait TeamRepository: Send + Sync {
    async fn upsert(&self, team: &Team) -> RepoResult<()>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<Team>>;
    async fn list_all(&self) -> RepoResult<Vec<Team>>;
}

#[async_trait]
pub trait TournamentGroupRepository: Send + Sync {
    async fn upsert(&self, group: &TournamentGroup) -> RepoResult<()>;
    async fn list_all(&self) -> RepoResult<Vec<TournamentGroup>>;
    /// Idempotently associate a team with a tournament group.
    async fn assign_team(&self, assignment: &TournamentGroupAssignment) -> RepoResult<()>;
    async fn list_teams_in_group(&self, group_id: Uuid) -> RepoResult<Vec<Team>>;
}

#[async_trait]
pub trait StandingsPredictionRepository: Send + Sync {
    async fn upsert(&self, prediction: &GroupStandingsPrediction) -> RepoResult<()>;
    async fn list_by_pickem(
        &self,
        pickem_group_id: Uuid,
    ) -> RepoResult<Vec<GroupStandingsPrediction>>;
    async fn record_points(&self, prediction_id: Uuid, points: i32) -> RepoResult<()>;
}

#[async_trait]
pub trait BestThirdsPredictionRepository: Send + Sync {
    /// Replace the user's best-thirds picks for a pickem with a fresh set.
    /// The implementation deletes the existing rows and re-inserts so the
    /// invariant "exactly the supplied set is stored" is atomic.
    async fn replace(&self, prediction: &BestThirdsPrediction) -> RepoResult<()>;
    async fn list_by_user(
        &self,
        pickem_group_id: Uuid,
        user_id: TelegramUserId,
    ) -> RepoResult<BestThirdsPrediction>;
    /// Persist the per-(pickem, user) aggregate score to `best_thirds_scoring`.
    async fn record_score(
        &self,
        pickem_group_id: Uuid,
        user_id: TelegramUserId,
        points: i32,
    ) -> RepoResult<()>;
}
