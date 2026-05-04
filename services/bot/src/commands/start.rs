use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/start`. Greets the user and (if invoked in a group chat) registers
/// them as a member of that group's pickem.
#[allow(dead_code)] // body lands with feature #1
pub async fn handle(
    _state: &AppState,
    _ctx: &CommandContext,
) -> anyhow::Result<HandlerOutcome> {
    todo!("commands::start::handle")
}
