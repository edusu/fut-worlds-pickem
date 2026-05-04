use shared::Config;

use crate::app_state::AppState;

/// Run the long-polling getUpdates loop.
///
/// Skeleton: in real life this calls `frankenstein::AsyncApi::get_updates`
/// in a loop with a growing offset, builds a `CommandContext` from each
/// update, parses the text via `commands::Command::parse`, dispatches via
/// `commands::dispatch`, and forwards the resulting `HandlerOutcome` to the
/// Telegram client. Errors are reported via `tracing` so a single bad
/// update never tears down the loop.
pub async fn run(_config: Config, _state: AppState) -> anyhow::Result<()> {
    // TODO: build the Frankenstein client, run the long-polling loop, dispatch.
    todo!("update_loop::run")
}
