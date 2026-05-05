use crate::app_state::AppState;
use crate::commands::{CommandContext, HandlerOutcome};

/// Handle `/play`. Replies with a message that carries an inline `web_app`
/// button opening the Mini App, so the user can submit predictions for the
/// active round of the current group.
#[allow(dead_code)] // body lands with feature #4 (Mini App entry)
pub async fn handle(_state: &AppState, _ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    todo!("commands::play::handle")
}
