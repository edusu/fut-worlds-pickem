use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/help`. Sends a static text listing the available commands and a
/// one-line description of each, mirroring the canonical list in CLAUDE.md.
#[allow(dead_code)] // body lands with feature #1
pub async fn handle(
    _state: &AppState,
    _ctx: &CommandContext,
) -> anyhow::Result<HandlerOutcome> {
    todo!("commands::help::handle")
}
