-- Owner: services/events (scheduler tick — feature #8)
-- Table: prediction_reminders
-- Idempotency ledger: one row per (user, match) reminder DM the events
-- scheduler has delivered. The presence of a row means the bot
-- successfully sent the DM via Telegram — failures (403 from a user who
-- never started the bot, network errors) intentionally do not insert,
-- so the scheduler retries on the next tick until the deadline window
-- closes.
--
-- ON DELETE CASCADE on both FKs so dropping a user or replaying a match
-- ingest does not strand orphan reminder rows.

CREATE TABLE prediction_reminders (
    user_id   BIGINT NOT NULL REFERENCES users(telegram_id) ON DELETE CASCADE,
    match_id  UUID   NOT NULL REFERENCES matches(id)        ON DELETE CASCADE,
    sent_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (user_id, match_id)
);

-- Reverse lookup for the scheduler's "give me unreminded users for this
-- match" anti-join. The composite PK already covers the
-- (user_id, match_id) point lookup.
CREATE INDEX prediction_reminders_match_idx
    ON prediction_reminders (match_id);
