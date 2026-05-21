//! `/play` — send a message with an inline `web_app` button that opens
//! the Mini App. Works the same in private chats and groups.

use shared::report_to_anyhow;

use crate::app_state::AppState;
use crate::commands::{mini_app_button_row, CommandContext, HandlerOutcome};

pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    let buttons = mini_app_button_row(&state.miniapp_url);

    state
        .telegram
        .send_text_with_buttons(
            ctx.chat_id.0,
            "Tap the button below to open the Mini App and submit your predictions.",
            &buttons,
        )
        .await
        .map_err(report_to_anyhow)?;

    Ok(HandlerOutcome::Silent)
}
