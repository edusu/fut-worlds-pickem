//! Slash-command handlers. One file per command keeps each handler focused
//! and easy to test in isolation. Routing of raw message text to a typed
//! variant lives in `parser`; the `dispatch` function below turns that
//! variant into the matching handler call.

pub mod help;
pub mod join;
pub mod new_pickem;
pub mod parser;
pub mod play;
pub mod ranking;
pub mod start;

pub use parser::Command;

use domain::{TelegramChatId, User};

/// Per-message context built from a parsed Telegram update. Carries the
/// invoking user (already shaped as a domain `User`) and the chat where the
/// command was issued. Anything broader — repos, telegram client — lives in
/// `AppState` and is injected separately.
#[derive(Debug, Clone)]
pub struct CommandContext {
    pub chat_id: TelegramChatId,
    /// Title of the chat as Telegram reports it. `None` in private chats.
    pub chat_title: Option<String>,
    /// The Telegram user that issued the command, already mapped to the
    /// domain shape. Handlers upsert this into `users` before any FK-bound
    /// write (group_members, predictions).
    pub user: User,
}

/// What the dispatcher returns to the update loop. The update loop is what
/// actually calls Telegram, so handlers stay pure: they decide what to say,
/// not how to deliver it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandlerOutcome {
    /// Send a plain-text reply back to the same chat.
    Reply(String),
    /// Acknowledge silently — the command was understood but produces no
    /// chat output (e.g. logging-only paths).
    #[allow(dead_code)] // first user lands with feature #1 wiring
    Silent,
}

/// Dispatch a parsed `Command` to its handler.
///
/// The exhaustive `match` guarantees the compiler complains the moment a new
/// `Command` variant is added without a corresponding handler arm.
#[allow(dead_code)] // wired up by the update loop in feature #1
pub async fn dispatch(
    state: &crate::app_state::AppState,
    ctx: &CommandContext,
    command: Command,
) -> anyhow::Result<HandlerOutcome> {
    match command {
        Command::Start => start::handle(state, ctx).await,
        Command::NewPickem => new_pickem::handle(state, ctx).await,
        Command::Join => join::handle(state, ctx).await,
        Command::Play => play::handle(state, ctx).await,
        Command::Ranking => ranking::handle(state, ctx).await,
        Command::Help => help::handle(state, ctx).await,
    }
}
