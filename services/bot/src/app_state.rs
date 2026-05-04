//! Shared runtime state injected into every command handler.
//!
//! Handlers receive `&AppState` and pull whatever ports they need. The
//! repository fields are `Arc<dyn …>` so:
//!
//! 1. Cheap clones — passing state across tokio tasks is just an Arc bump.
//! 2. Test doubles — a unit test can build an `AppState` from in-memory mocks
//!    of the same trait without touching Postgres.
//!
//! Add a new field here when (and only when) more than one handler needs it.
//! One-shot dependencies belong inside the handler that uses them.

use std::sync::Arc;

use domain::repository::{GroupRepository, ScoringRuleRepository, UserRepository};
use persistence::{PgGroupRepository, PgScoringRuleRepository, PgUserRepository};
use sqlx::PgPool;

/// Shared dependencies handed to every command handler.
#[derive(Clone)]
pub struct AppState {
    pub users: Arc<dyn UserRepository>,
    pub groups: Arc<dyn GroupRepository>,
    pub scoring_rules: Arc<dyn ScoringRuleRepository>,
}

impl AppState {
    /// Build the production state from a live Postgres pool. The pool is
    /// cloned into each repository — `PgPool` is internally `Arc`-shared, so
    /// this does not multiply real connections.
    pub fn new(pool: PgPool) -> Self {
        Self {
            users: Arc::new(PgUserRepository::new(pool.clone())),
            groups: Arc::new(PgGroupRepository::new(pool.clone())),
            scoring_rules: Arc::new(PgScoringRuleRepository::new(pool)),
        }
    }
}
