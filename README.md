# Fut Worlds Pickem

A Telegram bot + Mini App for football-prediction pickems during the 2026 World
Cup. Friends create a pickem inside their Telegram group, predict scores via the
Mini App, and the system scores predictions automatically in the database.
Ranking is pull-only — the bot answers `/ranking` and the Mini App has a ranking
page; no push notifications to the chat (see [CLAUDE.md](./CLAUDE.md) §Current
scope for what's deferred).

## Architecture

```
                      ┌──────────────────────┐
                      │  football-data.org   │
                      └──────────┬───────────┘
                                 │ HTTPS
                                 ▼
   ┌───────────┐    NATS   ┌──────────┐    SQL     ┌───────────┐
   │   bot     │◀─────────▶│  events  │◀──────────▶│ Postgres  │
   │ (Telegram │           │ (ingest, │            │           │
   │  updates) │           │  cron,   │            └─────▲─────┘
   └─────┬─────┘           │  scorer) │                  │
         │ HTTPS           └────┬─────┘            SQL   │
         │ Bot API              │ NATS                   │
         ▼                      ▼                        │
   ┌───────────┐           ┌──────────┐                  │
   │ Telegram  │           │   api    │──────────────────┘
   │  servers  │◀──────────│  (HTTP)  │
   └─────┬─────┘   webhook └────▲─────┘
         │                      │ HTTPS (Mini App)
         │                      │
         ▼                      │
   ┌───────────────────────────┴──────────────────────────────┐
   │  React Mini App (frontend/miniapp), served as static SPA │
   └──────────────────────────────────────────────────────────┘
```

Three Rust services share a Cargo workspace and a Postgres instance. Cross-service
signals travel as JSON events over NATS subjects prefixed with `pickem.*`. The
React Mini App is a separate package outside the workspace (polyglot monorepo).

## Quick start

1. **Clone and configure**
   ```sh
   git clone <repo> fut-worlds-pickem && cd fut-worlds-pickem
   cp .env.example .env
   # fill TELEGRAM_BOT_TOKEN and FOOTBALL_API_KEY
   ```

2. **Bring up the dev dependencies** (Postgres + NATS + Jaeger)
   ```sh
   just dev-up
   ```

3. **Run migrations**
   ```sh
   just migrate
   ```

4. **Start the services** (in separate shells, or `just run-all`)
   ```sh
   just bot      # long-polling Telegram bot
   just api      # HTTP API on http://localhost:8080
   just events   # ingester + scheduler + scorer
   ```

5. **Run the Mini App**
   ```sh
   just front-install   # one-time
   just front
   ```

6. **Quality gates**
   ```sh
   just lint    # cargo fmt --check + clippy -D warnings
   just test    # cargo test --workspace
   ```

## Repository layout

```
fut-worlds-pickem/
├── Cargo.toml                 # Workspace root
├── justfile                   # Common dev commands
├── infra/
│   ├── docker-compose.yml      # Postgres + NATS + Jaeger (deps only)
│   └── docker-compose.full.yml # Full stack: deps + services + Mini App
├── migrations/                # SQL migrations (numeric prefix order)
├── crates/
│   ├── domain/                # Pure types + repository traits
│   ├── persistence/           # sqlx adapters
│   ├── messaging/             # NATS contract: events + topics
│   ├── sports-client/         # football-data.org wrapper
│   ├── telegram-client/       # Telegram trait + frankenstein impl
│   └── shared/                # Config, tracing init, error types
├── services/
│   ├── bot/                   # Telegram updates + notification consumer
│   ├── api/                   # HTTP API for the Mini App
│   └── events/                # Ingester + scheduler + scorer
└── frontend/miniapp/          # React + Vite Telegram Mini App
```

- Architecture, conventions, and how-tos: [CLAUDE.md](./CLAUDE.md)
- Pending features and where to touch the code: [ROADMAP.md](./ROADMAP.md)

## License

Dual-licensed under MIT or Apache-2.0.
