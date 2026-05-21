//! `/ranking` — render the current standings of the pickem bound to this chat.
//!
//! Plain text only — the `TelegramClient` trait does not surface a
//! `parse_mode` knob today, and Markdown would need separate escaping for
//! every user name. A numbered list with an em-dash is unambiguous either
//! way and renders cleanly across Telegram clients.

use std::fmt::Write as _;

use shared::report_to_anyhow;

use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

const EMPTY_REPLY: &str = "Ranking is empty — no points awarded yet.";

pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    let group = match state
        .groups
        .find_by_chat(ctx.chat_id)
        .await
        .map_err(report_to_anyhow)?
    {
        Some(g) => g,
        None => {
            return Ok(HandlerOutcome::Reply(
                "No pickem in this group yet. Run /new_pickem first.".into(),
            ));
        }
    };

    let entries = state
        .ranking
        .group_ranking(group.id)
        .await
        .map_err(report_to_anyhow)?;

    if entries.is_empty() {
        return Ok(HandlerOutcome::Reply(EMPTY_REPLY.into()));
    }

    // Header keeps the trailing blank line so the numbered list visually
    // separates from the title in the Telegram client.
    let mut body = format!("Ranking — {}\n", group.name);
    for (idx, entry) in entries.iter().enumerate() {
        // `write!` into a `String` is infallible — the `_` swallows the
        // mandatory `Result` from `std::fmt::Write` without a `.unwrap()`.
        let _ = write!(
            body,
            "\n{}. {} — {} pts",
            idx + 1,
            entry.first_name,
            entry.total_points
        );
    }

    Ok(HandlerOutcome::Reply(body))
}
