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
    BestThirdsPrediction, Group, GroupMember, GroupStandingsPrediction, KnockoutPhase,
    KnockoutPhaseState, KnockoutStage, Match, Prediction, ScoringRule, Team, TelegramChatId,
    TelegramUserId, Tournament, TournamentGroup, TournamentGroupAssignment, TournamentGroupState,
    User,
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
pub trait TournamentRepository: Send + Sync {
    /// Insert or refresh a tournament by `external_id`. The ingester calls
    /// this on first boot to seed e.g. the World Cup 2026 row before any
    /// groups, phases, or matches exist.
    async fn upsert(&self, tournament: &Tournament) -> RepoResult<()>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<Tournament>>;
    /// Look a tournament up by its upstream code, e.g. `"WC"`. Used by the
    /// bootstrap to translate "the v1 tournament" into a UUID.
    async fn find_by_external_id(&self, external_id: &str) -> RepoResult<Option<Tournament>>;
}

#[async_trait]
pub trait MatchRepository: Send + Sync {
    async fn upsert(&self, match_: &Match) -> RepoResult<()>;
    /// Group-stage matches in a single tournament group. Returns rows
    /// where `tournament_group_id = $1` ordered by `kickoff_at`.
    async fn list_by_tournament_group(&self, group_id: Uuid) -> RepoResult<Vec<Match>>;
    /// Knockout matches in a single phase. Returns rows where
    /// `knockout_phase_id = $1` ordered by `kickoff_at`.
    async fn list_by_knockout_phase(&self, phase_id: Uuid) -> RepoResult<Vec<Match>>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<Match>>;
}

#[async_trait]
pub trait PredictionRepository: Send + Sync {
    /// Insert or update a prediction. On UPDATE (i.e. an edit on an existing
    /// (user, pickem, match) triple) the implementation must set
    /// `was_changed = true` so the scorer applies the re-pick penalty. The
    /// first insert leaves `was_changed = false`.
    async fn upsert(&self, prediction: &Prediction) -> RepoResult<()>;
    /// Every prediction targeting a specific match, across all pickems.
    /// Used by the scorer when a match finishes — each prediction is
    /// scored against its pickem's `scoring_rule_id`.
    async fn list_by_match(&self, match_id: Uuid) -> RepoResult<Vec<Prediction>>;
    /// Every prediction a user has ever made, across pickems. Reserved for
    /// admin / cross-pickem analytics; the per-pickem Mini App view uses
    /// `list_by_user_in_pickem` instead.
    async fn list_by_user(&self, user_id: TelegramUserId) -> RepoResult<Vec<Prediction>>;
    /// All predictions inside a single pickem, across users. Used by the
    /// ranking aggregator.
    async fn list_by_pickem(&self, pickem_group_id: Uuid) -> RepoResult<Vec<Prediction>>;
    /// A user's predictions inside a single pickem. The natural query for
    /// the Mini App's "show me my picks for this pickem" view: indexable
    /// against `predictions_user_pickem_match_unique` and avoids loading
    /// other users' rows.
    async fn list_by_user_in_pickem(
        &self,
        user_id: TelegramUserId,
        pickem_group_id: Uuid,
    ) -> RepoResult<Vec<Prediction>>;
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
    async fn find_by_country_code(&self, country_code: &str) -> RepoResult<Option<Team>>;
    async fn list_all(&self) -> RepoResult<Vec<Team>>;
}

#[async_trait]
pub trait TournamentGroupRepository: Send + Sync {
    async fn upsert(&self, group: &TournamentGroup) -> RepoResult<()>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<TournamentGroup>>;
    /// Look up by the natural key `(tournament_id, name)`. Used by the
    /// bootstrap to detect already-seeded groups.
    async fn find_by_name(
        &self,
        tournament_id: Uuid,
        name: &str,
    ) -> RepoResult<Option<TournamentGroup>>;
    async fn list_by_tournament(&self, tournament_id: Uuid) -> RepoResult<Vec<TournamentGroup>>;
    /// Active groups in a tournament currently accepting predictions:
    /// state `open` and deadline still in the future. The deadline check
    /// matters because the api enforces it at write time (no scheduler
    /// flips rows to `closed` in v1).
    async fn list_active(&self, tournament_id: Uuid) -> RepoResult<Vec<TournamentGroup>>;
    async fn set_state(&self, group_id: Uuid, state: TournamentGroupState) -> RepoResult<()>;
    /// Groups whose deadline has passed but state is still `open`.
    /// Returned in ascending `deadline_at` order.
    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<TournamentGroup>>;
    /// Idempotently associate a team with a tournament group. The
    /// composite FK on the join table prevents inconsistent assignments
    /// (group from a different tournament than the assignment claims).
    async fn assign_team(&self, assignment: &TournamentGroupAssignment) -> RepoResult<()>;
    async fn list_teams_in_group(&self, group_id: Uuid) -> RepoResult<Vec<Team>>;
}

#[async_trait]
pub trait KnockoutPhaseRepository: Send + Sync {
    async fn upsert(&self, phase: &KnockoutPhase) -> RepoResult<()>;
    async fn find(&self, id: Uuid) -> RepoResult<Option<KnockoutPhase>>;
    /// Look up by the natural key `(tournament_id, stage)`. Used by the
    /// bootstrap to detect already-seeded phases.
    async fn find_by_stage(
        &self,
        tournament_id: Uuid,
        stage: KnockoutStage,
    ) -> RepoResult<Option<KnockoutPhase>>;
    /// All phases of a tournament ordered by `position` ASC. Drives the
    /// bracket render in the Mini App.
    async fn list_by_tournament(&self, tournament_id: Uuid) -> RepoResult<Vec<KnockoutPhase>>;
    /// Phases currently accepting predictions: state `open` and deadline
    /// still in the future.
    async fn list_active(&self, tournament_id: Uuid) -> RepoResult<Vec<KnockoutPhase>>;
    async fn set_state(&self, phase_id: Uuid, state: KnockoutPhaseState) -> RepoResult<()>;
    /// Phases whose deadline has passed but state is still `open`.
    async fn list_due_for_close(&self, before: DateTime<Utc>) -> RepoResult<Vec<KnockoutPhase>>;
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
