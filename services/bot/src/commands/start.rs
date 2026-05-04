use shared::report_to_anyhow;

use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/start`. Upserts the invoking user (so we have a row to FK against
/// when they later run `/join`) and replies with a short welcome that points
/// at `/help` for the full command list.
pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    state
        .users
        .upsert(&ctx.user)
        .await
        .map_err(report_to_anyhow)?;

    Ok(HandlerOutcome::Reply(format!(
        "Welcome, {}! This bot runs World Cup pickems with your friends. \
         Use /help to see what's available.",
        ctx.user.first_name
    )))
}
