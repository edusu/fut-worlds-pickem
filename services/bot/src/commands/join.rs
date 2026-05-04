//! `/join` — register the invoking user as a member of the chat's pickem.
//!
//! Idempotent: re-running for a user that is already a member is a silent
//! no-op at the storage layer (`ON CONFLICT (group_id, user_id) DO NOTHING`).

use chrono::Utc;
use domain::GroupMember;
use shared::report_to_anyhow;

use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/join`. Adds the invoking user to the pickem of the current
/// chat. If no pickem exists yet, the reply nudges the owner to run
/// `/new_pickem` first.
pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    state
        .users
        .upsert(&ctx.user)
        .await
        .map_err(report_to_anyhow)?;

    let group = match state
        .groups
        .find_by_chat(ctx.chat_id)
        .await
        .map_err(report_to_anyhow)?
    {
        Some(g) => g,
        None => {
            return Ok(HandlerOutcome::Reply(
                "No pickem in this group yet. The group admin should run /new_pickem first."
                    .to_string(),
            ));
        }
    };

    state
        .groups
        .add_member(&GroupMember {
            group_id: group.id,
            user_id: ctx.user.telegram_id,
            joined_at: Utc::now(),
        })
        .await
        .map_err(report_to_anyhow)?;

    Ok(HandlerOutcome::Reply(format!(
        "You're in '{}'. Open the Mini App to enter your predictions.",
        group.name
    )))
}
