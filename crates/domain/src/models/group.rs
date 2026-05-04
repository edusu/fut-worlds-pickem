use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::user::TelegramUserId;

/// Strong wrapper around a Telegram chat id (negative for groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramChatId(pub i64);

/// A pickem group: 1:1 with a Telegram chat.
///
/// `owner_id` records who created the pickem (the user that ran
/// `/new_pickem`). It carries no privilege today — everyone is treated
/// equally — and is kept purely for audit/lineage.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub telegram_chat_id: TelegramChatId,
    pub name: String,
    pub owner_id: TelegramUserId,
    pub scoring_rule_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Membership row linking a user to a group. The relation has no role:
/// every member is treated equally for predictions, scoring, and ranking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub group_id: Uuid,
    pub user_id: TelegramUserId,
    pub joined_at: DateTime<Utc>,
}
