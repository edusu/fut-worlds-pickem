use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::user::TelegramUserId;

/// Strong wrapper around a Telegram chat id (negative for groups).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TelegramChatId(pub i64);

/// Role a user has within a pickem group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GroupRole {
    Owner,
    Player,
}

/// A pickem group: 1:1 with a Telegram chat.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    pub id: Uuid,
    pub telegram_chat_id: TelegramChatId,
    pub name: String,
    pub owner_id: TelegramUserId,
    pub scoring_rule_id: Uuid,
    pub created_at: DateTime<Utc>,
}

/// Membership row linking a user to a group with a role.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GroupMember {
    pub group_id: Uuid,
    pub user_id: TelegramUserId,
    pub role: GroupRole,
    pub joined_at: DateTime<Utc>,
}
