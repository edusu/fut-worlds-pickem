//! Shared runtime state injected into every command handler.
//!
//! Handlers receive `&AppState` and pull whatever ports they need.
use std::sync::Arc;

use domain::repository::{
    GroupRepository, RankingRepository, ScoringRuleRepository, UserRepository,
};
use persistence::{
    PgGroupRepository, PgRankingRepository, PgScoringRuleRepository, PgUserRepository,
};
use sqlx::PgPool;
use telegram_client::TelegramClient;

/// Shared dependencies handed to every command handler.
#[derive(Clone)]
pub struct AppState {
    pub users: Arc<dyn UserRepository>,
    pub groups: Arc<dyn GroupRepository>,
    pub scoring_rules: Arc<dyn ScoringRuleRepository>,
    pub ranking: Arc<dyn RankingRepository>,
    /// Outbound Telegram channel. Handlers send messages, inline keyboards
    /// and chat-pin requests through this port; the update loop also uses
    /// it to translate `HandlerOutcome::Reply` into a Telegram send.
    pub telegram: Arc<dyn TelegramClient>,
    /// Public Mini App URL rendered into the `web_app` inline button
    /// produced by `/new_pickem` and `/play`.
    pub miniapp_url: Arc<String>,
}

impl AppState {
    /// Build the production state from a live Postgres pool and an arbitrary
    /// Telegram client. The pool is cloned into each repository — `PgPool`
    /// is internally `Arc`-shared, so this does not multiply real
    /// connections.
    pub fn new(pool: PgPool, telegram: Arc<dyn TelegramClient>, miniapp_url: Arc<String>) -> Self {
        Self {
            users: Arc::new(PgUserRepository::new(pool.clone())),
            groups: Arc::new(PgGroupRepository::new(pool.clone())),
            scoring_rules: Arc::new(PgScoringRuleRepository::new(pool.clone())),
            ranking: Arc::new(PgRankingRepository::new(pool)),
            telegram,
            miniapp_url,
        }
    }
}
