# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

**Roadmap:** [ROADMAP.md](./ROADMAP.md) is the source of truth for what's
pending and where to touch the code. This file (CLAUDE.md) covers stable
architecture and conventions; ROADMAP.md covers changing priorities.

## Project summary

Fut Worlds Pickem is a Telegram bot + Mini App for football-prediction pickems
during the 2026 World Cup. Friends create a pickem inside their Telegram group,
predict scores via the Mini App, and the system scores predictions automatically
in the database. Ranking is **pull-only for v1**: the bot replies to `/ranking`
and the Mini App renders a ranking page on demand — no push notifications to
the chat.

## Architecture

Three Rust microservices share a Cargo workspace plus a single Postgres
database. They communicate over NATS JetStream subjects prefixed with `pickem.*`.
A separate React Mini App (Vite + TypeScript, **outside** the Cargo workspace,
inside `frontend/miniapp/`) is consumed only by the API service.

```
┌──────────┐  NATS ┌──────────┐  SQL  ┌──────────┐
│   bot    │◀────▶│  events  │◀────▶│ Postgres │
└────┬─────┘       └────┬─────┘       └────┬─────┘
     │                  │                  │
     │ Telegram         │ football-data    │ SQL
     ▼                  ▼                  │
 [Telegram]         [upstream]         ┌───┴──────┐
                                       │   api    │◀───── Mini App (HTTPS)
                                       └──────────┘
```

- **`services/bot`** — owns Telegram I/O. Long-polling update loop dispatching
  slash commands. It is the *only* service that calls the Telegram Bot API
  (social convention). The `event_consumers::notify` NATS consumer subscribed
  to `pickem.notification.requested` is currently a stub — push-to-chat is
  deferred (see "Current scope" below).
- **`services/api`** — Axum HTTP server consumed by the Mini App. Every
  protected route is gated by `middleware::auth::verify_init_data` (HMAC-SHA256
  over Telegram's signed `initData`). The endpoint also enforces "round still
  open" at submission time, since v1 has no background job to close rounds at
  the deadline.
- **`services/events`** — ingester (polls football-data.org for fixtures and
  results) and scorer (consumes `pickem.match.finished` and writes
  `predictions.points_awarded`). The scheduler module is scaffolded but inert
  in v1 — deadlines are enforced at write time, not by a cron job.

### Library crates

- `crates/domain` — pure types and repository traits. **No I/O dependencies.**
- `crates/persistence` — `sqlx`-backed Postgres adapters for the domain traits.
- `crates/messaging` — versioned event payloads + topic constants + a
  `Publisher` / `Subscriber` pair. **Every cross-service signal goes through
  this crate.**
- `crates/sports-client` — football-data.org wrapper.
- `crates/telegram-client` — `TelegramClient` trait + `frankenstein` impl.
- `crates/shared` — `Config::from_env`, `tracing::init`, common error types.

### Database table ownership

Postgres is shared but each table has a single logical owner — only that
service writes; others may read. Owner is documented in the migration header.

| Tables                           | Owner            |
|----------------------------------|------------------|
| `users`, `groups`, `group_members` | `services/bot`   |
| `rounds`, `matches`              | `services/events`|
| `predictions`                    | `services/api`   |
| `scoring_rules`                  | `services/events`|

## Commands

All daily commands run through `just`. Install with `brew install just` (or see
`https://github.com/casey/just`). The `justfile` is the source of truth.

| Goal                              | Command                                  |
|-----------------------------------|------------------------------------------|
| Spin up Postgres + NATS + Jaeger  | `just dev-up`                            |
| Tear down dev deps                | `just dev-down`                          |
| Run migrations                    | `just migrate`                           |
| Run a single service              | `just bot` / `just api` / `just events`  |
| Run all three services            | `just run-all`                           |
| Mini App dev server               | `just front` (after `just front-install`)|
| Type-check Rust workspace         | `just check`                             |
| Lint (fmt + clippy `-D warnings`) | `just lint`                              |
| Auto-format                       | `just fmt`                               |
| Run all Rust tests                | `just test`                              |
| Run a single test                 | `cargo test -p <crate> <test_name>`      |
| Refresh sqlx offline cache        | `just sqlx-prepare`                      |

Frontend (run inside `frontend/miniapp/`):

| Goal                | Command            |
|---------------------|--------------------|
| Type-check          | `pnpm typecheck`   |
| Production build    | `pnpm build`       |
| Lint                | `pnpm lint`        |

CI runs `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
`cargo test --workspace`, and `pnpm typecheck && pnpm build`. The Postgres
service is provided as a sidecar; CI exports placeholder values for
`TELEGRAM_BOT_TOKEN` and `FOOTBALL_API_KEY` so `Config::from_env()` succeeds in
test contexts that exercise it.

## Conventions

- **Edition / toolchain:** Rust 2021, MSRV 1.80, `rust-toolchain.toml` pins
  stable + rustfmt + clippy.
- **Formatting:** `rustfmt.toml` sets `max_width = 100`. Run `just fmt` before
  committing.
- **Lints:** `cargo clippy --workspace --all-targets -- -D warnings` must pass.
  When stubbing out unimplemented surface, prefer **localized**
  `#[allow(dead_code)]` over crate-wide allows so the lint starts firing again
  the moment real callers appear.
- **Comments and docs in code:** English only, even if discussion is in Spanish.
  Public items get a short docstring; explain *why* in inline comments, not
  *what*.
- **Async traits:** use `async-trait`. Keep traits in `domain`; impls live in
  `persistence` (or whichever adapter crate fits).
- **Versioned events:** every event in `messaging::events` is a `#[serde(tag =
  "version")]` enum starting at `V1`. **Never delete or repurpose a variant** —
  add a new one and migrate.
- **Versions:** prefer the latest stable from crates.io / npm. Don't pin to
  unverified versions. The workspace `[workspace.dependencies]` is the single
  place to bump shared deps; per-crate manifests pull with
  `tokio = { workspace = true }`.

## Environment variables

`Config::from_env()` (in `crates/shared/src/config.rs`) is the single source of
truth for what a service needs at startup. Required:

- `DATABASE_URL` — `postgres://user:pass@host:5432/db`
- `NATS_URL` — `nats://host:4222`
- `TELEGRAM_BOT_TOKEN` — from @BotFather
- `FOOTBALL_API_KEY` — football-data.org token
- `API_BIND_ADDR` — e.g. `0.0.0.0:8080`

Optional:

- `TELEGRAM_WEBHOOK_URL` — leave empty for long polling
- `OTEL_EXPORTER_OTLP_ENDPOINT` — e.g. `http://localhost:4317` for Jaeger
- `OTEL_SERVICE_NAMESPACE` — resource attribute
- `RUST_LOG` — tracing filter (default `info`)

Copy `.env.example` to `.env` for local dev. The `justfile` has
`set dotenv-load := true`, so any recipe sees the same values.

## How to add a new event to the messaging system

1. Add a versioned variant to (or create) the right enum in
   `crates/messaging/src/events.rs`. Start with `V1(MyEventV1)`.
2. Add the topic constant in `crates/messaging/src/topics.rs`. Use the
   `pickem.<area>.<verb>` pattern.
3. **Producer side:** inject a `Publisher` (from `messaging::Publisher`) into
   the call site and call `publisher.publish(topics::MY_TOPIC, &event).await?`.
4. **Consumer side:** create a `Subscriber<MyEvent>` with
   `Subscriber::subscribe(&nats_client, topics::MY_TOPIC).await?`, then loop
   over `sub.next().await`.
5. The bot is the **only** service allowed to consume
   `pickem.notification.requested` and convert it into a Telegram send.
   Producers that need to talk to a chat publish a `NotificationRequested`
   rather than calling the Telegram API directly. **v1 status:** no producer
   currently emits this event and the consumer is a stub — the contract is
   defined but the path is dormant until push-to-chat is reintroduced.

## How to add a new database migration

1. Create `migrations/NNNN_<descriptive_name>.sql`. The numeric prefix must
   strictly increase (`sqlx migrate` orders by it).
2. Top of file: a header comment naming the **owner service** for any new
   tables, even if a column reuses an existing table — colocates ownership with
   the schema change.
3. `just migrate` to apply against the configured `DATABASE_URL`.
4. After schema or query changes, run `just sqlx-prepare` if you've added
   compile-time–verified queries, and commit the resulting `.sqlx/` cache so CI
   can build offline.

## How to start the whole stack locally

```sh
cp .env.example .env           # fill TELEGRAM_BOT_TOKEN + FOOTBALL_API_KEY
just dev-up                    # Postgres + NATS + Jaeger
just migrate                   # apply all migrations

# In separate shells (or `just run-all`):
just bot
just api
just events

# In another shell:
just front-install             # one-time
just front                     # http://localhost:5173
```

Jaeger UI is at `http://localhost:16686`. NATS monitoring at
`http://localhost:8222`.

## Telegram bot commands

Canonical list registered with BotFather (and synced via `setMyCommands` —
see ROADMAP feature #7). All descriptions are English; keep this table in
sync with the actual handlers in `services/bot/src/commands/`.

| Command       | Description                                            |
|---------------|--------------------------------------------------------|
| `/start`      | Show welcome message and quick help                    |
| `/new_pickem` | Create a pickem in this group (admin only)             |
| `/join`       | Join the pickem in this group                          |
| `/play`       | Open the Mini App to enter your predictions            |
| `/ranking`    | Show the current standings in this group               |
| `/help`       | Show available commands and how to play                |

Paste-ready BotFather format:

```
start - Show welcome message and quick help
new_pickem - Create a pickem in this group (admin only)
join - Join the pickem in this group
play - Open the Mini App to enter your predictions
ranking - Show the current standings in this group
help - Show available commands and how to play
```

## Current scope (v1)

What's in:

- Bot receives `/start`, `/new_pickem`, `/join`, `/play`, `/ranking`, `/help`
  and replies synchronously to the invoking chat.
- Mini App lets users submit predictions (auth via signed `initData`).
- Events service ingests fixtures from football-data.org, detects finished
  matches, and writes `predictions.points_awarded` via the scorer.
- Ranking is computed on demand: `/ranking` and `GET /api/groups/{id}/ranking`
  both run a `SUM(points_awarded) GROUP BY user_id` against Postgres at
  request time.

What's deferred (scaffolded but dormant):

- Push notifications to the chat. The `pickem.notification.requested`
  contract, the bot's `event_consumers::notify` consumer, and the
  `RoundDeadlineApproaching` / `RoundClosed` / `RoundScored` event variants
  are all declared but no producer emits them.
- The cron scheduler (`tokio-cron-scheduler`) — the module is scaffolded,
  no jobs are registered. Round closing is enforced at write time by the API
  validating `round.state == 'open' AND now < round.deadline_at`.

When reintroducing push-to-chat: emit `NotificationRequested` from the relevant
producer, wire the bot's notify consumer to render each `NotificationTemplate`
variant, and only then add scheduler jobs that publish the timed templates.

## Things to know that aren't obvious from the code

- The Mini App `auth` middleware is currently an **insecure stub** that only
  checks the header is present — it must implement full HMAC-SHA256 validation
  before anything ships. Look for the `INSECURE STUB` comment in
  `services/api/src/middleware/auth.rs`.
- `frontend/miniapp/` is intentionally NOT a Cargo workspace member — the
  workspace `members` glob excludes the `frontend/` directory.
- `docker-compose.yml` extends services from `docker-compose.dev.yml` rather
  than duplicating them; edit dev first, full picks it up.
- Service Dockerfiles use `cargo-chef` for dependency caching, so source-only
  changes don't redownload crates.io packages.
