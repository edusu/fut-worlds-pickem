use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Strong wrapper around a Telegram user id.
///
/// Telegram identifies users by an i64. We wrap it so it cannot be confused
/// with other numeric ids in function signatures.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramUserId(pub i64);

/// A Telegram user known to the system.
///
/// We intentionally store only the public fields the bot needs to address the
/// user. Avatars and bios are NOT stored — privacy first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    pub telegram_id: TelegramUserId,
    pub username: Option<String>,
    pub first_name: String,
    pub language_code: Option<String>,
    pub created_at: DateTime<Utc>,
}
