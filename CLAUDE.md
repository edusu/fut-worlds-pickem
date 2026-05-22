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

- **`services/bot`** — owns the Telegram update path: long-polling loop
  dispatching slash commands, replying through a shared `TelegramClient`,
  and surfacing the Mini App via `setMyCommands` + `setChatMenuButton` at
  startup. The `event_consumers::notify` NATS consumer subscribed to
  `pickem.notification.requested` is currently a stub — push-to-chat is
  deferred (see "Current scope" below). `services/events` also holds a
  `TelegramClient` so it can send pre-deadline reminder DMs directly; that
  is the one deliberate exception to the "only the bot talks to Telegram"
  rule (chosen so #8 did not need to reactivate the `NotificationRequested`
  contract).
- **`services/api`** — Axum HTTP server consumed by the Mini App. Every
  protected route is gated by `middleware::auth::verify_init_data` (HMAC-SHA256
  over Telegram's signed `initData`). The endpoint also enforces "the parent
  submission window (tournament_group or knockout_phase) is still open" at
  submission time, since v1 has no background job to close them at the deadline.
- **`services/events`** — three concurrent loops: (a) the **ingester** polls
  football-data.org for fixtures and emits `pickem.match.finished` on
  `→ finished` transitions; (b) the **scorer** (live `MatchFinished`
  consumer plus a periodic `reconcile` loop) writes
  `predictions.points_awarded` and the end-of-group-stage
  `group_standings_predictions` / `best_thirds_scoring` rows; (c) the
  **reminder scheduler** in `scheduler/reminders.rs` DMs members with
  pending predictions in the configured pre-deadline window. Submission
  deadlines are still enforced at write time by the API — there is no
  cron job that flips a parent's `state` column.

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

| Tables                                                                 | Owner            |
|------------------------------------------------------------------------|------------------|
| `users`, `groups`, `group_members`                                     | `services/bot`   |
| `tournaments`, `teams`, `tournament_groups`, `knockout_phases`, `tournament_group_teams`, `matches` | `services/events`|
| `predictions`, `group_standings_predictions`, `best_thirds_predictions`, `best_thirds_scoring`      | `services/api` (writes); `services/events` writes `points_awarded` only |
| `scoring_rules`                                                        | `services/events`|

The fixtures graph (`tournaments → teams → tournament_groups → knockout_phases → tournament_group_teams → matches`) is consolidated in migration `0002`; migration `0005` is a documented placeholder. Match parents are polymorphic: every `matches` row points at exactly one of `tournament_group_id` or `knockout_phase_id`, enforced by the `matches_parent_xor` CHECK. Submission deadlines and lifecycle (`open`/`closed`/`scored`) live on `tournament_groups` and `knockout_phases` directly — there is no `rounds` table.

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

- `MINIAPP_URL` — public HTTPS URL of the Mini App that the bot's menu
  button and `/new_pickem` / `/play` inline buttons point to. Defaults
  to `http://localhost:5173` so a fresh checkout works without further
  configuration; pin a distinct HTTPS value in production.
- `MINIAPP_ORIGIN` — origin allowed by the API's CORS layer for the
  Mini App. Defaults to `http://localhost:5173`.
- `TELEGRAM_WEBHOOK_URL` — leave empty for long polling
- `OTEL_EXPORTER_OTLP_ENDPOINT` — e.g. `http://localhost:4317` for Jaeger
- `OTEL_SERVICE_NAMESPACE` — resource attribute
- `RUST_LOG` — tracing filter (default `info`)
- `SCORER_RECONCILE_INTERVAL_SECS` — cadence of the events scorer's
  reconcile loop in seconds (default 600). The loop catches matches
  finished while the scorer was down, group standings pending
  transition, and best-thirds waiting for the R16 bracket.
- `REMINDER_TICK_INTERVAL_SECS` — cadence of the events reminder
  scheduler tick in seconds (default 600).
- `REMINDER_WINDOW_HOURS_MIN` / `REMINDER_WINDOW_HOURS_MAX` — bounds (in
  hours from `now`) of the deadline window for which reminder DMs are
  dispatched. Defaults to 6 and 24 — i.e. a member who has not submitted
  is DMed when the matching parent's `deadline_at` is between 6h and 24h
  away.

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
4. After **any** schema or query change, run `just sqlx-prepare` and commit
   the resulting `.sqlx/` cache. The persistence layer uses `sqlx::query!` /
   `query_as!` macros (compile-time validation against the live schema) for
   every repo whose SQL is a literal string, so the cache is required to
   build offline. CI runs `cargo sqlx prepare --check` inside `just lint`
   and fails if the committed cache disagrees with the source. The two
   exceptions still on `sqlx::query()` (no compile-time validation) are
   `tournament_groups.rs`, `knockout_phases.rs`, and `scoring_rules.rs` —
   those build their SQL via `format!("{SELECT_*_COLUMNS} ...")` and the
   macro requires literal strings.

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
- Mini App lets users submit predictions, gated by full HMAC-SHA256
  validation of Telegram's signed `initData` in
  `services/api/src/middleware/auth.rs`.
- Mini App entry points: persistent DM menu button
  (`setChatMenuButton`), pinned `/new_pickem` confirmation with an
  inline `web_app` button, and `/play` slash command producing the
  same button on demand.
- Events service ingests fixtures from football-data.org, detects
  finished matches, scores them via the live consumer (plus a
  reconcile loop that catches anything missed while the scorer was
  down), and emits end-of-group-stage standings + best-thirds scores
  in the same pass.
- Pre-deadline reminder DMs from
  `services/events/src/scheduler/reminders.rs` — grouped per user so a
  member with N pending predictions receives one DM, not N, and
  idempotent across ticks via the `prediction_reminders` table.
- Ranking is computed on demand: `/ranking` and `GET /api/groups/{id}/ranking`
  both run a `SUM(points_awarded) GROUP BY user_id` against Postgres at
  request time.

What's deferred (scaffolded but dormant):

- **Push notifications to a chat (group-level messages, not DMs).** The
  `pickem.notification.requested` contract, the bot's
  `event_consumers::notify` consumer, and the
  `SubmissionDeadlineApproaching` / `SubmissionWindowClosed` /
  `SubmissionWindowScored` event variants are all declared but no
  producer emits them. (These were the `Round*` events before the
  rounds-to-phases refactor.) Pre-deadline DMs — the only live
  notification path — bypass this contract entirely and write through
  the events service's own `TelegramClient`.
- **The `tokio-cron-scheduler`-based job runner** that would publish
  those deferred notification events. The module is scaffolded, no
  jobs are registered. Submission closing is enforced at write time by
  the API validating `parent.state == 'open' AND now < parent.deadline_at`,
  where `parent` is the relevant `tournament_groups` or
  `knockout_phases` row. (The live reminder scheduler uses a plain
  `tokio::time::interval`, not `tokio-cron-scheduler`.)
- `updated_at` columns on mutation-prone tables (matches, predictions,
  groups, phases). Out for v1; reintroduce with a `BEFORE UPDATE` trigger
  when audit becomes a requirement.
- Per-stage knockout deadlines. The schema supports each `knockout_phase`
  having its own `deadline_at`; v1 sets all 6 to the same first-knockout
  kickoff. Switching to per-stage is a seed-side change with no schema
  migration.

When reintroducing push-to-chat: emit `NotificationRequested` from the relevant
producer, wire the bot's notify consumer to render each `NotificationTemplate`
variant, and only then add scheduler jobs that publish the timed templates.

## Things to know that aren't obvious from the code

- The Mini App auth middleware derives the HMAC secret once at startup
  from `TELEGRAM_BOT_TOKEN` (it is the *inner* key of
  `HMAC(SHA256("WebAppData"), bot_token)`) and caches it in middleware
  state, so each request only runs the inner HMAC pass. Rotating the
  bot token therefore requires restarting the API service — there is
  no live reload of the cached secret.
- Bot entry points encode the pickem id as a `?pickem=<uuid>` query
  parameter on the Mini App launch URL — built by
  `commands::build_miniapp_url` in `services/bot/src/commands/mod.rs`
  and consumed by `getPickemId()` in
  `frontend/miniapp/src/lib/pickem.ts`. The query-param route exists
  because inline `web_app` buttons cannot populate
  `initDataUnsafe.start_param` — Telegram only sets that field when
  the Mini App is opened via a Direct Link (`t.me/<bot>/<app>?startapp=X`).
  The DM menu button, and `/play` in a chat with no pickem, still
  pass the bare URL; the Mini App then renders the "open from your
  group" hint instead of the predictions UI.
- `frontend/miniapp/` is intentionally NOT a Cargo workspace member — the
  workspace `members` glob excludes the `frontend/` directory.
- All compose files live in `infra/`. `infra/docker-compose.yml` is the
  deps-only file (Postgres + NATS + Jaeger) used by `just dev-up`.
  `infra/docker-compose.full.yml` extends from it and adds the three Rust
  services + the Mini App for E2E/staging — edit the deps file first, full
  picks the changes up via `extends`.
- Service Dockerfiles use `cargo-chef` for dependency caching, so source-only
  changes don't redownload crates.io packages.
