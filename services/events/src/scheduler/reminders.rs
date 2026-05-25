//! Pre-deadline reminder DMs.
//!
//! One tick: find every `(user, match)` pair whose parent's deadline
//! falls in the configured window, that has no submitted prediction
//! across the user's pickems and no prior reminder row. Group the
//! pairs by user, mark them as sent (so the next tick skips them),
//! then send a single DM. This mark-before-send order gives
//! at-most-once delivery: a crash between mark and send loses one
//! DM rather than causing duplicates.
//!
//! Per-user grouping matters because all 6 matches of a World Cup
//! group share one parent deadline — without batching, a user with
//! no group-stage predictions would receive 6 simultaneous DMs.

use std::sync::Arc;

use chrono::{Duration, Utc};
use domain::repository::PredictionReminderRepository;
use domain::PendingReminder;
use telegram_client::{TelegramClient, TelegramError};
use tracing::{info, warn};

/// Dependencies the reminder tick needs. Carried as a struct so tests
/// can construct it with mock implementations without forming a
/// dependency on the live `services/events` wiring.
pub struct ReminderDeps {
    pub reminders: Arc<dyn PredictionReminderRepository>,
    pub telegram: Arc<dyn TelegramClient>,
    pub window_min_hours: u64,
    pub window_max_hours: u64,
}

/// Run a single tick. Public so `scheduler::run` can call it from its
/// interval loop and the events binary can also invoke it on demand
/// (smoke tests, manual ops). Returns the number of DMs *successfully*
/// delivered — `403 Forbidden` users are counted as skipped.
pub async fn run_once(deps: &ReminderDeps) -> anyhow::Result<usize> {
    let now = Utc::now();
    let pending = deps
        .reminders
        .list_pending(
            now,
            Duration::hours(deps.window_min_hours as i64),
            Duration::hours(deps.window_max_hours as i64),
        )
        .await
        .map_err(shared::report_to_anyhow)?;

    if pending.is_empty() {
        return Ok(0);
    }

    // Group by user so the DM batches all pending matches into one
    // message. `list_pending` already orders by `user_id`, so a single
    // linear pass suffices.
    let mut delivered = 0usize;
    for (user_id, batch) in group_by_user(pending) {
        // Mark before send: at-most-once. A crash between mark and
        // send loses one DM; the alternative (send-then-mark) risks
        // duplicates on transient Postgres failures.
        deps.reminders
            .mark_sent_batch(&batch)
            .await
            .map_err(shared::report_to_anyhow)?;

        let text = render_reminder(batch.len());
        match deps.telegram.send_text(user_id.0, &text).await {
            Ok(()) => {
                delivered += 1;
            }
            Err(report) if matches!(report.current_context(), TelegramError::ForbiddenByUser) => {
                warn!(
                    user_id = user_id.0,
                    pending = batch.len(),
                    "skipping reminder: user has not opened a DM with the bot"
                );
            }
            Err(report) => {
                warn!(
                    user_id = user_id.0,
                    pending = batch.len(),
                    error = ?report,
                    "reminder DM failed"
                );
            }
        }
    }

    info!(delivered, "reminder tick complete");
    Ok(delivered)
}

/// Group an already-`user_id`-sorted slice of `PendingReminder`s into
/// `(user_id, Vec<PendingReminder>)` runs, allocating only one Vec per
/// distinct user.
fn group_by_user(
    pending: Vec<PendingReminder>,
) -> Vec<(domain::TelegramUserId, Vec<PendingReminder>)> {
    let mut grouped: Vec<(domain::TelegramUserId, Vec<PendingReminder>)> = Vec::new();
    for p in pending {
        match grouped.last_mut() {
            Some((uid, batch)) if *uid == p.user_id => batch.push(p),
            _ => grouped.push((p.user_id, vec![p])),
        }
    }
    grouped
}

/// Render the DM body. Plain text only — directs users back to their
/// group chat where `/play` can resolve the pickem context.
fn render_reminder(pending_count: usize) -> String {
    format!(
        "Reminder: you have {pending_count} pending prediction{plural} \
         before the next deadline. Head to your group chat and tap /play \
         to open the Mini App and lock them in.",
        plural = if pending_count == 1 { "" } else { "s" },
    )
}

#[cfg(test)]
mod tests {
    use uuid::Uuid;

    use super::*;
    use domain::TelegramUserId;

    #[test]
    fn group_by_user_collapses_consecutive_runs() {
        let m1 = Uuid::new_v4();
        let m2 = Uuid::new_v4();
        let m3 = Uuid::new_v4();
        let input = vec![
            PendingReminder {
                user_id: TelegramUserId(1),
                match_id: m1,
            },
            PendingReminder {
                user_id: TelegramUserId(1),
                match_id: m2,
            },
            PendingReminder {
                user_id: TelegramUserId(2),
                match_id: m3,
            },
        ];
        let grouped = group_by_user(input);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].0, TelegramUserId(1));
        assert_eq!(grouped[0].1.len(), 2);
        assert_eq!(grouped[1].0, TelegramUserId(2));
        assert_eq!(grouped[1].1.len(), 1);
    }

    #[test]
    fn render_reminder_uses_singular_for_one() {
        let one = render_reminder(1);
        assert!(one.contains("1 pending prediction "), "{one}");
        assert!(one.contains("group chat"), "{one}");
        let many = render_reminder(3);
        assert!(many.contains("3 pending predictions "), "{many}");
    }
}
