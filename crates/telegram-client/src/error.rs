//! Error types for the Telegram Bot API client.
//!
//! Source errors from `frankenstein` (or any swap-in implementation) are not
//! embedded — callers chain them via `error_stack::ResultExt::change_context`
//! and add chat id / method name as `attach_with` context.

use error_stack::Report;
use thiserror::Error;

/// High-level taxonomy of Telegram API failures.
#[derive(Debug, Error)]
pub enum TelegramError {
    /// The Telegram Bot API rejected the request (4xx/5xx, invalid token, etc.).
    #[error("Telegram API error")]
    Api,
    /// Local request building or response parsing failed.
    #[error("Telegram protocol error")]
    Protocol,
    /// The user has never initiated the chat with the bot, or has
    /// blocked / deactivated it — Telegram returns `403 Forbidden`.
    /// Surfaced as a typed variant so the reminder scheduler can
    /// silently skip the recipient instead of bailing out on the tick.
    #[error("Telegram forbade the bot from messaging this user")]
    ForbiddenByUser,
}

/// Convenience alias for a fully-formed report of a Telegram error.
pub type TelegramReport = Report<TelegramError>;

/// Result type for every method on the `TelegramClient` trait.
pub type TelegramResult<T> = Result<T, TelegramReport>;
