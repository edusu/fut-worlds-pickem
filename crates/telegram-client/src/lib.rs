//! Trait abstraction over the Telegram Bot API plus a `frankenstein`-backed
//! implementation. Anything that needs to talk to Telegram should depend on
//! the trait, not on `frankenstein` directly — that keeps tests trivial
//! (just hand-roll a mock impl). Errors live in `crate::error`.

pub mod error;
pub mod throttle;

pub use error::{TelegramError, TelegramReport, TelegramResult};
pub use throttle::ThrottledTelegramClient;

use async_trait::async_trait;
use error_stack::ResultExt;
use frankenstein::client_reqwest::Bot;
use frankenstein::methods::SendMessageParams;
use frankenstein::AsyncTelegramApi;

/// One inline-keyboard button. Either a callback (data routed back to the
/// bot) or a URL (opens a Mini App / external link).
#[derive(Debug, Clone)]
pub enum Button {
    Callback { text: String, data: String },
    Url { text: String, url: String },
}

#[async_trait]
pub trait TelegramClient: Send + Sync {
    /// Send a plain text message to a chat.
    async fn send_text(&self, chat_id: i64, text: &str) -> TelegramResult<()>;

    /// Send a message with an inline keyboard. `buttons` is laid out as rows.
    async fn send_text_with_buttons(
        &self,
        chat_id: i64,
        text: &str,
        buttons: &[Vec<Button>],
    ) -> TelegramResult<()>;

    /// Acknowledge a callback query (clears the loading spinner on the
    /// client). `text` is an optional toast.
    async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: Option<&str>,
    ) -> TelegramResult<()>;

    /// Register the bot's webhook URL (no-op if using long polling).
    async fn set_webhook(&self, url: &str) -> TelegramResult<()>;
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
    /// `None`. Wraps any frankenstein error as `TelegramError::Api`.
    async fn send_text(&self, chat_id: i64, text: &str) -> TelegramResult<()> {
        let params = SendMessageParams::builder()
            .chat_id(chat_id)
            .text(text.to_string())
            .build();

        self.bot
            .send_message(&params)
            .await
            .change_context(TelegramError::Api)?;

        Ok(())
    }

    async fn send_text_with_buttons(
        &self,
        _chat_id: i64,
        _text: &str,
        _buttons: &[Vec<Button>],
    ) -> TelegramResult<()> {
        // TODO: convert `buttons` to InlineKeyboardMarkup and send.
        todo!("FrankensteinClient::send_text_with_buttons")
    }

    async fn answer_callback_query(
        &self,
        _callback_query_id: &str,
        _text: Option<&str>,
    ) -> TelegramResult<()> {
        todo!("FrankensteinClient::answer_callback_query")
    }

    async fn set_webhook(&self, _url: &str) -> TelegramResult<()> {
        todo!("FrankensteinClient::set_webhook")
    }
}
