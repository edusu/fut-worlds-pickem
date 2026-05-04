use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/ranking`. Renders the current standings of the pickem group as a
/// Markdown table.
#[allow(dead_code)] // body lands with feature #6
pub async fn handle(
    _state: &AppState,
    _ctx: &CommandContext,
) -> anyhow::Result<HandlerOutcome> {
    todo!("commands::ranking::handle")
}
