//! Long-polling update loop. Pulls Telegram updates with a growing offset,
//! turns each one into a `CommandContext`, runs it through the dispatcher
//! on a bounded worker pool, and forwards the resulting `HandlerOutcome`
//! back to Telegram.
//!
//! Concurrency model: the loop only awaits `get_updates` and `pool.submit`
//! — the per-update handling itself runs on `rust_utils::concurrency::WorkerPool`,
//! which spawns up to `POOL_CAPACITY` tasks concurrently and applies
//! backpressure on `submit` once the cap is hit. That keeps a slow handler
//! from blocking the next poll while still bounding the work in flight, so
//! a flood of `/ranking` calls cannot exhaust the Postgres pool or starve
//! the rest of the service.
//!
//! Delivery semantics: the offset is advanced **before** `submit`, so a
//! crash between submit and execution loses that update. This is
//! intentional — handlers are idempotent on persisted state but their
//! Telegram replies are not (the second `/new_pickem` would say "already
//! exists" rather than the original success message), so at-most-once is
//! the friendlier UX trade-off in v1. A future at-least-once design would
//! introduce an outbox + dedupe key rather than moving the offset advance
//! into the spawned task.

use std::num::{NonZeroU32, NonZeroUsize};
use std::sync::Arc;
use std::time::Duration;

use error_stack::Report;
use frankenstein::methods::GetUpdatesParams;
use frankenstein::types::{AllowedUpdate, ChatType};
use frankenstein::updates::{Update, UpdateContent};
use frankenstein::AsyncTelegramApi;
use rust_utils::concurrency::{Retry, WorkerPool};
use rust_utils::error::UtilsError;
use shared::Config;
use telegram_client::FrankensteinClient;
use tracing::{debug, error, info_span, warn, Instrument};

use crate::app_state::AppState;
use crate::commands::{self, Command, CommandContext, HandlerOutcome};

// ─── Polling tuning ─────────────────────────────────────────────────────────
//
// Long-poll timeout. Telegram's HTTP call hangs for up to this many seconds
// waiting for new updates, so the loop is essentially free while the chat is
// idle.
const POLL_TIMEOUT_SECS: u32 = 30;

// Maximum number of *consecutive* failed `get_updates` calls before we give
// up and propagate the error to the runtime. A success resets the counter.
const MAX_RETRIES: u32 = 5;

// Initial backoff after the first failure. Doubles on each subsequent
// consecutive failure (1s, 2s, 4s, 8s, 16s) and is capped at `MAX_BACKOFF`.
const BASE_BACKOFF: Duration = Duration::from_secs(1);
const MAX_BACKOFF: Duration = Duration::from_secs(30);

// Maximum number of update handlers in flight at any given time. Sized so
// the worst-case fan-out cannot exhaust the Postgres connection pool (10–20
// connections by default) while still smoothing out bursts of activity.
const POOL_CAPACITY: usize = 8;

// Canned replies. Kept as constants so the wording stays consistent across
// every handler error / unknown-command nudge.
const ERROR_REPLY: &str = "Sorry, something went wrong. Please try again.";
const UNKNOWN_COMMAND_REPLY: &str = "Try /help to see available commands.";

/// Drive the long-polling loop until the runtime is shut down.
///
/// Errors that affect a single update are logged inside `process_update`
/// and never surface here — only an outright failure to talk to Telegram
/// (network down for too long, bad token) bubbles up and tears down the
/// service. Transient `get_updates` errors are retried with jittered
/// exponential backoff up to `MAX_RETRIES` consecutive attempts before the
/// loop gives up.
pub async fn run(
    _config: Config,
    state: AppState,
    telegram: Arc<FrankensteinClient>,
) -> anyhow::Result<()> {
    // Resolve our own username once. `Command::parse` uses it to accept
    // `/cmd@MyBot` while rejecting `/cmd@SomeoneElse`.
    let bot_username = telegram
        .bot()
        .get_me()
        .await?
        .result
        .username
        .ok_or_else(|| anyhow::anyhow!("getMe returned a bot without a username"))?;
    let bot_username = Arc::new(bot_username);

    // Build the bounded worker pool. The handler closure is called on every
    // submitted update; it clones the per-call state so the closure itself
    // remains `Fn` (each invocation gets a fresh, independent copy).
    let mut pool = {
        let pool_state = state.clone();
        let pool_username = Arc::clone(&bot_username);
        WorkerPool::new(
            NonZeroUsize::new(POOL_CAPACITY).expect("POOL_CAPACITY is non-zero"),
            move |update: Update| {
                let state = pool_state.clone();
                let bot_username = Arc::clone(&pool_username);
                async move {
                    process_update(state, bot_username, update).await;
                }
            },
        )
    };

    // ─── Polling loop ────────────────────────────────────────────────────
    //
    // `offset = None` on the first call. After processing each update we
    // advance to `update.update_id + 1` so Telegram does not re-deliver it.
    //
    // `get_updates` is wrapped in `Retry` so transient network blips are
    // absorbed silently; only after `MAX_RETRIES` consecutive failures does
    // the loop bubble the error up and let `tokio::try_join!` in `main`
    // tear the service down — by then the failure is no longer transient
    // (revoked token, sustained outage) and silently looping would just
    // hide the problem.
    let mut offset: Option<i64> = None;

    loop {
        let params = GetUpdatesParams::builder()
            .maybe_offset(offset)
            .timeout(POLL_TIMEOUT_SECS)
            .allowed_updates(vec![AllowedUpdate::Message])
            .build();

        // Retry::run runs the closure once per attempt, applying exponential
        // backoff with full jitter between attempts. The closure has to map
        // frankenstein's error into a `UtilsResult` so Retry can inspect it;
        // the final `UtilsError::RetryExhausted` (if attempts are exceeded)
        // is converted back to `anyhow::Error` for the function return.
        let response = Retry::new()
            .name("get_updates")
            .max_attempts(NonZeroU32::new(MAX_RETRIES).expect("MAX_RETRIES is non-zero"))
            .base_backoff(BASE_BACKOFF)
            .max_backoff(MAX_BACKOFF)
            .jitter(true)
            .run(|_attempt| {
                let telegram = Arc::clone(&telegram);
                let params = params.clone();
                async move {
                    telegram.bot().get_updates(&params).await.map_err(|e| {
                        Report::new(UtilsError::Network).attach(format!("get_updates failed: {e}"))
                    })
                }
            })
            .await
            .map_err(shared::report_to_anyhow)?;

        for update in response.result {
            // Always advance the offset, even before submitting the update
            // for processing — leaving it stale would make Telegram
            // redeliver a poison-pill update forever.
            offset = Some(i64::from(update.update_id) + 1);

            // `submit` awaits a free worker slot. While the pool is
            // saturated, polling is paused — that backpressure is the
            // whole point of bounding the pool, and is preferable to
            // unbounded `tokio::spawn` which would let a misbehaving
            // burst exhaust Postgres connections.
            pool.submit(update).await;
        }
    }
}

/// Handle a single Telegram update end-to-end on a worker task: filter,
/// build the domain-shaped context, parse the command, dispatch, and
/// forward the outcome to Telegram. Never returns an error to the caller —
/// failures are logged and (when reasonable) communicated to the user via
/// a generic reply.
async fn process_update(state: AppState, bot_username: Arc<String>, update: Update) {
    // Empty placeholders for `chat_id` and `command` so the span carries
    // them once they are known. `tracing` requires the field set to be
    // declared at span construction time; recording an undeclared field
    // is a silent no-op.
    let span = info_span!(
        "update",
        update_id = update.update_id,
        chat_id = tracing::field::Empty,
        command = tracing::field::Empty,
    );

    async move {
        // ─── 1. Surface the message ─────────────────────────────────────
        //
        // `allowed_updates(vec![AllowedUpdate::Message])` filters
        // server-side, but `update.content` is still a wide enum, so we
        // still need to pattern-match to recover the Message payload.
        // Anything else (edited messages, callback queries, …) is
        // out-of-scope for v1 — drop it silently.
        let UpdateContent::Message(message) = update.content else {
            return;
        };
        let message = *message;

        let chat_id = message.chat.id;
        tracing::Span::current().record("chat_id", chat_id);

        // We only care about plain-text commands. Captions on photos/videos
        // and entities-only messages are out of scope for v1.
        let Some(text) = message.text.as_deref() else {
            return;
        };

        // ─── 2. Identify the sender ─────────────────────────────────────
        //
        // Anonymous group admins post with `from = None`. We cannot upsert
        // a fantom user nor attribute the command to anyone, so the only
        // safe action is to skip — but we log it at WARN so that "the bot
        // ignored me" reports can be diagnosed quickly.
        let Some(from) = message.from else {
            warn!(chat_id, "anonymous admin message — skipping");
            return;
        };

        // Other bots can technically send `/new_pickem` and similar bare
        // commands; ignore them so two bots in a group cannot loop into
        // each other and so we never seed `users` with a bot account.
        if from.is_bot {
            debug!(from_id = from.id, "message from a bot — skipping");
            return;
        }

        // ─── 3. Parse the command ───────────────────────────────────────
        let command = match Command::parse(text, &bot_username) {
            Some(cmd) => cmd,
            None => {
                // In a private chat the user is talking *to* the bot
                // directly, so a hint is friendlier than silence. In a
                // group, replies to non-command chatter would be spam.
                if matches!(message.chat.type_field, ChatType::Private) {
                    if let Err(e) = state
                        .telegram
                        .send_text(chat_id, UNKNOWN_COMMAND_REPLY)
                        .await
                    {
                        warn!(error = ?e, "failed to send unknown-command nudge");
                    }
                }
                return;
            }
        };
        tracing::Span::current().record("command", tracing::field::debug(&command));

        // ─── 4. Build the dispatch context ──────────────────────────────
        //
        // The frankenstein `User` has u64 ids; the domain wraps an i64.
        // Real Telegram ids are well within i53, so the cast is safe in
        // practice. `created_at` is set to "now" but PgUserRepository does
        // not refresh it on UPSERT, so a re-sender does not lose their
        // historical row.
        let domain_user = domain::User {
            telegram_id: domain::TelegramUserId(from.id as i64),
            username: from.username.clone(),
            first_name: from.first_name.clone(),
            language_code: from.language_code.clone(),
            created_at: chrono::Utc::now(),
        };
        let ctx = CommandContext {
            chat_id: domain::TelegramChatId(chat_id),
            chat_title: message.chat.title.clone(),
            user: domain_user,
        };

        // ─── 5. Dispatch and forward the outcome ────────────────────────
        match commands::dispatch(&state, &ctx, command).await {
            Ok(HandlerOutcome::Reply(text)) => {
                if let Err(e) = state.telegram.send_text(chat_id, &text).await {
                    error!(error = ?e, "failed to deliver reply to Telegram");
                }
            }
            Ok(HandlerOutcome::Silent) => {}
            Err(e) => {
                error!(error = ?e, "command handler failed");
                // Best-effort generic reply. If even this fails (Telegram
                // outage) we just log — the offset is already advanced so
                // there is no infinite-redelivery risk.
                if let Err(e2) = state.telegram.send_text(chat_id, ERROR_REPLY).await {
                    warn!(error = ?e2, "failed to deliver generic error reply");
                }
            }
        }
    }
    .instrument(span)
    .await;
}
