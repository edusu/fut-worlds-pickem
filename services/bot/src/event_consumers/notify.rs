use messaging::{topics, NotificationRequested, Subscriber};
use shared::Config;
use tracing::info;

/// Subscribe to `pickem.notification.requested` and translate each payload
/// into a Telegram message via the `telegram-client` trait. The bot is the
/// ONLY consumer of this topic — the rule is enforced socially, not in code.
pub async fn run(_config: Config, nats: async_nats::Client) -> anyhow::Result<()> {
    let mut sub: Subscriber<NotificationRequested> =
        Subscriber::subscribe(&nats, topics::NOTIFICATION_REQUESTED)
            .await
            .map_err(shared::report_to_anyhow)?;

    info!(
        topic = topics::NOTIFICATION_REQUESTED,
        "notify consumer ready"
    );

    while let Some(_event) = sub.next().await {
        // TODO: pattern-match on the event variant and call the configured
        // TelegramClient (send_text / send_text_with_buttons).
        todo!("event_consumers::notify::run dispatch")
    }
    Ok(())
}
