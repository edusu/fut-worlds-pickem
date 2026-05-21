//! Trait abstraction over the Telegram Bot API plus a `frankenstein`-backed
//! implementation. Anything that needs to talk to Telegram should depend on
//! the trait, not on `frankenstein` directly — that keeps tests trivial
//! (just hand-roll a mock impl). Errors live in `crate::error`.

pub mod error;
pub mod throttle;

pub use error::{TelegramError, TelegramReport, TelegramResult};
pub use throttle::ThrottledTelegramClient;

use async_trait::async_trait;
use error_stack::Report;
use frankenstein::client_reqwest::Bot;
use frankenstein::methods::{
    PinChatMessageParams, SendMessageParams, SetChatMenuButtonParams, SetMyCommandsParams,
};
use frankenstein::types::{
    BotCommand, InlineKeyboardButton, InlineKeyboardMarkup, MenuButton, MenuButtonWebApp,
    ReplyMarkup, WebAppInfo,
};
use frankenstein::AsyncTelegramApi;

/// Telegram HTTP code returned when the bot cannot DM the user — either
/// the user never started the bot, has blocked it, or has been
/// deactivated. The scheduler treats this as "skip and continue".
const TELEGRAM_FORBIDDEN_CODE: u64 = 403;

/// One inline-keyboard button. `Url` opens an external link in the in-app
/// browser; `WebApp` launches a Telegram Mini App (with signed `initData`)
/// — distinct because Telegram routes them to different fields on
/// `InlineKeyboardButton`.
#[derive(Debug, Clone)]
pub enum Button {
    Callback { text: String, data: String },
    Url { text: String, url: String },
    WebApp { text: String, url: String },
}

#[async_trait]
pub trait TelegramClient: Send + Sync {
    /// Send a plain text message to a chat.
    async fn send_text(&self, chat_id: i64, text: &str) -> TelegramResult<()>;

    /// Send a message with an inline keyboard. `buttons` is laid out as rows.
    /// Returns the `message_id` of the delivered message so callers can chain
    /// follow-up actions (e.g. pinning the same message).
    async fn send_text_with_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[Vec<Button>],
    ) -> TelegramResult<i32>;

    /// Acknowledge a callback query (clears the loading spinner on the
    /// client). `text` is an optional toast.
    async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> TelegramResult<()>;

    /// Register the bot's webhook URL (no-op if using long polling).
    async fn set_webhook(&self, url: &str) -> TelegramResult<()>;

    /// Replace the bot's slash-command menu (rendered next to `/` in every
    /// chat). `commands` is `(command, description)` without the leading
    /// `/`. Idempotent — safe to re-run at every startup.
    async fn set_my_commands(&self, commands: &[(&str, &str)]) -> TelegramResult<()>;

    /// Replace the bot's global DM menu button with a Mini App launcher
    /// pointing at `url`. Bot-global: no `chat_id` means every DM.
    async fn set_chat_menu_button(&self, text: &str, url: &str) -> TelegramResult<()>;

    /// Pin a message silently (`disable_notification = true`) so we do
    /// not ping every group member. Caller must treat failures as
    /// best-effort — the bot may lack `can_pin_messages`.
    async fn pin_chat_message(&self, chat_id: i64, message_id: i32) -> TelegramResult<()>;
}

/// `frankenstein`-backed implementation. Wraps a `frankenstein::Bot` (the
/// async HTTP client) and the raw bot token. The `Bot` itself is cheap to
/// clone — it holds a `reqwest::Client` internally — so cloning the
/// wrapper shares the underlying HTTP connection pool too.
#[derive(Clone)]
pub struct FrankensteinClient {
    token: String,
    bot: Bot,
}

impl FrankensteinClient {
    /// Build a client bound to the given Telegram bot token.
    pub fn new(token: impl Into<String>) -> Self {
        let token = token.into();
        let bot = Bot::new(&token);
        Self { token, bot }
    }

    /// Expose the token only to consumers that already proved they need it
    /// (e.g. for HMAC validation of `initData`).
    pub fn token(&self) -> &str {
        &self.token
    }

    pub fn bot(&self) -> &Bot {
        &self.bot
    }
}

#[async_trait]
impl TelegramClient for FrankensteinClient {
    /// Send a plain-text message. Builds `SendMessageParams` with `chat_id`
    /// and `text` only — formatting, threads and reply-markup default to
    /// `None`.
    ///
    /// Maps Telegram's `403 Forbidden` (user never started the bot,
    /// blocked it, or got deactivated) to the typed
    /// `TelegramError::ForbiddenByUser` so the reminder scheduler can
    /// pattern-match and skip the recipient; every other failure
    /// surfaces as `TelegramError::Api`.
    async fn send_text(&self, chat_id: i64, text: &str) -> TelegramResult<()> {
        let params = SendMessageParams::builder()
            .chat_id(chat_id)
            .text(text.to_string())
            .build();

        self.bot
            .send_message(&params)
            .await
            .map_err(map_frankenstein_error)?;
        Ok(())
    }

    async fn send_text_with_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[Vec<Button>],
    ) -> TelegramResult<i32> {
        let keyboard: Vec<Vec<InlineKeyboardButton>> = buttons
            .iter()
            .map(|row| row.iter().map(to_inline_button).collect())
            .collect();
        let markup = InlineKeyboardMarkup::builder()
            .inline_keyboard(keyboard)
            .build();

        let params = SendMessageParams::builder()
            .chat_id(chat_id)
            .text(text.to_string())
            .reply_markup(ReplyMarkup::InlineKeyboardMarkup(markup))
            .build();

        let response = self
            .bot
            .send_message(&params)
            .await
            .map_err(map_frankenstein_error)?;
        Ok(response.result.message_id)
    }

    async fn answer_callback_query(
        &self,
        _callback_query_id: &str,
        _text: Option<&str>,
    ) -> TelegramResult<()> {
        Err(error_stack::Report::new(TelegramError::Api)
            .attach("FrankensteinClient::answer_callback_query not implemented"))
    }

    async fn set_webhook(&self, _url: &str) -> TelegramResult<()> {
        Err(error_stack::Report::new(TelegramError::Api)
            .attach("FrankensteinClient::set_webhook not implemented"))
    }

    async fn set_my_commands(&self, commands: &[(&str, &str)]) -> TelegramResult<()> {
        let payload: Vec<BotCommand> = commands
            .iter()
            .map(|(command, description)| {
                BotCommand::builder()
                    .command(command.to_string())
                    .description(description.to_string())
                    .build()
            })
            .collect();
        let params = SetMyCommandsParams::builder().commands(payload).build();
        self.bot
            .set_my_commands(&params)
            .await
            .map_err(map_frankenstein_error)?;
        Ok(())
    }

    async fn set_chat_menu_button(&self, text: &str, url: &str) -> TelegramResult<()> {
        let web_app = WebAppInfo::builder().url(url.to_string()).build();
        let menu = MenuButton::WebApp(
            MenuButtonWebApp::builder()
                .text(text.to_string())
                .web_app(web_app)
                .build(),
        );
        // Omit `chat_id` to scope the change bot-globally (DMs only — the
        // menu button is a private-chat concept).
        let params = SetChatMenuButtonParams::builder().menu_button(menu).build();
        self.bot
            .set_chat_menu_button(&params)
            .await
            .map_err(map_frankenstein_error)?;
        Ok(())
    }

    async fn pin_chat_message(&self, chat_id: i64, message_id: i32) -> TelegramResult<()> {
        let params = PinChatMessageParams::builder()
            .chat_id(chat_id)
            .message_id(message_id)
            .disable_notification(true)
            .build();
        self.bot
            .pin_chat_message(&params)
            .await
            .map_err(map_frankenstein_error)?;
        Ok(())
    }
}

fn to_inline_button(button: &Button) -> InlineKeyboardButton {
    match button {
        Button::Callback { text, data } => InlineKeyboardButton::builder()
            .text(text.clone())
            .callback_data(data.clone())
            .build(),
        Button::Url { text, url } => InlineKeyboardButton::builder()
            .text(text.clone())
            .url(url.clone())
            .build(),
        Button::WebApp { text, url } => InlineKeyboardButton::builder()
            .text(text.clone())
            .web_app(WebAppInfo::builder().url(url.clone()).build())
            .build(),
    }
}

/// Map a `frankenstein::Error` into our typed taxonomy. The `Api(403)`
/// case is split out so callers can pattern-match on
/// `TelegramError::ForbiddenByUser` and skip the user; every other
/// variant collapses to `TelegramError::Api` with the source attached.
fn map_frankenstein_error(e: frankenstein::Error) -> Report<TelegramError> {
    if let frankenstein::Error::Api(ref resp) = e {
        if resp.error_code == TELEGRAM_FORBIDDEN_CODE {
            return Report::new(e).change_context(TelegramError::ForbiddenByUser);
        }
    }
    Report::new(e).change_context(TelegramError::Api)
}
