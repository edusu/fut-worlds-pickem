use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Static help text. Mirror of the canonical command list in CLAUDE.md;
/// keep both in sync when commands are added or removed.
const HELP_TEXT: &str = "\
Available commands:\n\
/start - Welcome message and quick help\n\
/new_pickem - Create a pickem in this group (run from the group)\n\
/join - Join the pickem in this group\n\
/play - Open the Mini App to enter your predictions\n\
/ranking - Show the current standings in this group\n\
/help - Show this message";

/// Handle `/help`. Pure: no DB, no external calls — just the static menu.
pub async fn handle(_state: &AppState, _ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    Ok(HandlerOutcome::Reply(HELP_TEXT.to_string()))
}
