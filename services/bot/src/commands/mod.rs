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
use telegram_client::Button;
use uuid::Uuid;

/// Label rendered on every inline button that launches the Mini App
/// (`/new_pickem` confirmation, `/play`). Kept here so the CTA stays
/// identical across handlers.
pub const OPEN_MINI_APP_LABEL: &str = "Open Mini App";

/// Build the Mini App launch URL for a given pickem.
///
/// Encodes the pickem id as a `?pickem=<uuid>` query parameter so the
/// Mini App can bind the session to that pickem on load. We can't use
/// Telegram's `start_param` (which would be the more native channel)
/// because Telegram only populates it for *Direct Link* Mini Apps
/// (`t.me/<bot>/<app>?startapp=X`); for the inline `web_app` buttons
/// the bot sends today, the launch URL is the only signal that
/// reaches the frontend.
pub fn build_miniapp_url(base: &str, pickem_id: Option<Uuid>) -> String {
    let Some(id) = pickem_id else {
        return base.to_string();
    };
    // Respect any query string the operator may already have pinned on
    // `MINIAPP_URL` (e.g. a feature-flag override) so we don't break
    // the URL with a duplicate `?`.
    let sep = if base.contains('?') { '&' } else { '?' };
    format!("{base}{sep}pickem={id}")
}

/// Single-button row that opens the Mini App at `url`. Both `/new_pickem`
/// and `/play` render exactly this keyboard.
pub fn mini_app_button_row(url: &str) -> Vec<Vec<Button>> {
    vec![vec![Button::WebApp {
        text: OPEN_MINI_APP_LABEL.to_string(),
        url: url.to_string(),
    }]]
}

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
    /// Acknowledge silently — the handler already produced its own chat
    /// output through `state.telegram` (e.g. `/new_pickem` sends an
    /// inline-keyboard message and pins it directly).
    Silent,
}

/// Dispatch a parsed `Command` to its handler.
///
/// The exhaustive `match` guarantees the compiler complains the moment a new
/// `Command` variant is added without a corresponding handler arm.
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
