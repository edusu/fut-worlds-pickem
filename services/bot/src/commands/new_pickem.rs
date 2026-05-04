//! `/new_pickem` — create a pickem bound to the current Telegram chat.
//!
//! Idempotent at the chat level: re-running in a chat that already has a
//! pickem replies "already exists" instead of creating a duplicate. The
//! `groups.telegram_chat_id` UNIQUE constraint is the second line of
//! defence in case of races between two concurrent invocations.

use chrono::Utc;
use domain::{Group, GroupMember};
use shared::report_to_anyhow;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/new_pickem`. Creates a new pickem group bound to the current
/// Telegram chat, with the invoking user as the owner, and registers that
/// user as the first member.
pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    // Persist the user first — `group_members.user_id` FKs to `users`, so
    // creating a pickem before the row exists would 23503-fail.
    state
        .users
        .upsert(&ctx.user)
        .await
        .map_err(report_to_anyhow)?;

    // Idempotency check before insert. A concurrent second `/new_pickem`
    // would still be caught by the UNIQUE constraint on telegram_chat_id,
    // but in the common single-runner case this gives a clean message.
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
        name: ctx.chat_title.clone().unwrap_or_else(|| "Pickem".to_string()),
        owner_id: ctx.user.telegram_id,
        scoring_rule_id: scoring_rule.id,
        created_at: now,
    };

    state.groups.create(&group).await.map_err(report_to_anyhow)?;

    state
        .groups
        .add_member(&GroupMember {
            group_id: group.id,
            user_id: ctx.user.telegram_id,
            joined_at: now,
        })
        .await
        .map_err(report_to_anyhow)?;

    Ok(HandlerOutcome::Reply(format!(
        "Pickem '{}' created. Tell the others to run /join.",
        group.name
    )))
}
