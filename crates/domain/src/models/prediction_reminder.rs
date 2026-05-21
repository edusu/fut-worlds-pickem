use chrono::{DateTime, Utc};
use uuid::Uuid;

use crate::TelegramUserId;

/// One delivered reminder DM. The presence of the row is the
/// idempotency token — the scheduler refuses to re-send for any
/// `(user_id, match_id)` pair that already has a row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PredictionReminder {
    pub user_id: TelegramUserId,
    pub match_id: Uuid,
    pub sent_at: DateTime<Utc>,
}

/// `(user_id, match_id)` pair surfaced by the scheduler when scanning
/// for due reminders. Not a row of any table — it lives in flight from
/// the "find pending reminders" query to the "send DM + mark sent"
/// transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingReminder {
    pub user_id: TelegramUserId,
    pub match_id: Uuid,
}
