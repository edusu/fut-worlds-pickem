//! `/new_pickem` — create a pickem bound to the current Telegram chat.
//!
//! Idempotent at the chat level: re-running in a chat that already has a
//! pickem replies "already exists" instead of creating a duplicate. The
//! `groups.telegram_chat_id` UNIQUE constraint is the second line of
//! defence in case of races between two concurrent invocations.

use chrono::Utc;
use domain::{Group, GroupMember};
use shared::report_to_anyhow;
use tracing::warn;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::commands::{mini_app_button_row, CommandContext, HandlerOutcome};

/// Handle `/new_pickem`. Creates a new pickem group bound to the current
/// Telegram chat, with the invoking user as the owner, and registers that
/// user as the first member.
///
/// On the create path the handler also sends a confirmation message with
/// an inline `web_app` button into the Mini App and pins that message in
/// the chat, so anyone joining later still sees the entry point. The pin
/// is best-effort: when the bot is not a chat admin Telegram returns an
/// error and we log + continue (the inline button on the unpinned
/// message still works).
pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    // Persist the user first — `group_members.user_id` FKs to `users`, so
    // creating a pickem before the row exists would 23503-fail.
    state
        .users
        .upsert(&ctx.user)
        .await
        .map_err(report_to_anyhow)?;

    // Idempotency check before insert. The UNIQUE constraint on
    // `telegram_chat_id` is the backstop against a concurrent second
    // `/new_pickem`; the early return here also skips the send+pin path
    // below so re-runs do not pin a duplicate message.
    if let Some(existing) = state
        .groups
        .find_by_chat(ctx.chat_id)
        .await
        .map_err(report_to_anyhow)?
    {
        return Ok(HandlerOutcome::Reply(format!(
            "This group already has a pickem ('{}'). Use /join to participate.",
            existing.name
        )));
    }

    let scoring_rule = state
        .scoring_rules
        .default_rule()
        .await
        .map_err(report_to_anyhow)?;

    let now = Utc::now();
    let group = Group {
        id: Uuid::new_v4(),
        telegram_chat_id: ctx.chat_id,
        // Until we route the chat title through the update parser, fall
        // back to a generic name. The owner can rename it later.
        name: ctx
            .chat_title
            .clone()
            .unwrap_or_else(|| "Pickem".to_string()),
        owner_id: ctx.user.telegram_id,
        scoring_rule_id: scoring_rule.id,
        created_at: now,
    };

    let owner_member = GroupMember {
        group_id: group.id,
        user_id: ctx.user.telegram_id,
        joined_at: now,
    };

    state
        .groups
        .create_with_owner(&group, &owner_member)
        .await
        .map_err(report_to_anyhow)?;

    let chat_id = ctx.chat_id.0;
    let body = format!(
        "Pickem '{}' created. Tap the button below to enter your predictions, \
         or tell the others to run /join first.",
        group.name
    );
    let buttons = mini_app_button_row(&state.miniapp_url);

    let message_id = state
        .telegram
        .send_text_with_buttons(chat_id, &body, &buttons)
        .await
        .map_err(report_to_anyhow)?;

    if let Err(e) = state.telegram.pin_chat_message(chat_id, message_id).await {
        warn!(
            chat_id,
            message_id,
            error = ?e,
            "could not pin pickem-confirmation message; bot is probably not admin"
        );
    }

    Ok(HandlerOutcome::Silent)
}
