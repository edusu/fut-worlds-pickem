//! Shared runtime state injected into every command handler.
//!
//! Handlers receive `&AppState` and pull whatever ports they need.
use std::sync::Arc;

use domain::repository::{GroupRepository, ScoringRuleRepository, UserRepository};
use persistence::{PgGroupRepository, PgScoringRuleRepository, PgUserRepository};
use sqlx::PgPool;
use telegram_client::TelegramClient;

/// Shared dependencies handed to every command handler.
#[derive(Clone)]
pub struct AppState {
    pub users: Arc<dyn UserRepository>,
    pub groups: Arc<dyn GroupRepository>,
    pub scoring_rules: Arc<dyn ScoringRuleRepository>,
    /// Outbound Telegram channel. Today the update loop calls it (to
    /// translate `HandlerOutcome::Reply` into a Telegram message); features
    /// #4/#7 will let handlers send messages with inline buttons / pin chat
    /// messages directly via this port without going back through the
    /// dispatcher.
    pub telegram: Arc<dyn TelegramClient>,
}

impl AppState {
    /// Build the production state from a live Postgres pool and an arbitrary
    /// Telegram client. The pool is cloned into each repository — `PgPool`
    /// is internally `Arc`-shared, so this does not multiply real
    /// connections.
    pub fn new(pool: PgPool, telegram: Arc<dyn TelegramClient>) -> Self {
        Self {
            users: Arc::new(PgUserRepository::new(pool.clone())),
            groups: Arc::new(PgGroupRepository::new(pool.clone())),
            scoring_rules: Arc::new(PgScoringRuleRepository::new(pool)),
            telegram,
        }
    }
}
