//! `/play` — send a message with an inline `web_app` button that opens
//! the Mini App. Works the same in private chats and groups.

use shared::report_to_anyhow;

use crate::app_state::AppState;
use crate::commands::{build_miniapp_url, mini_app_button_row, CommandContext, HandlerOutcome};

pub async fn handle(state: &AppState, ctx: &CommandContext) -> anyhow::Result<HandlerOutcome> {
    // Resolve the pickem bound to this chat, if any. In a group with a
    // `/new_pickem`'d pickem the button lands the user directly on its
    // predictions; in a private chat (or a group with no pickem yet)
    // the button passes a bare URL and the Mini App renders its
    // "open from your group" hint.
    let pickem_id = state
        .groups
        .find_by_chat(ctx.chat_id)
        .await
        .map_err(report_to_anyhow)?
        .map(|g| g.id);
    let miniapp_url = build_miniapp_url(&state.miniapp_url, pickem_id);
    let buttons = mini_app_button_row(&miniapp_url);

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
