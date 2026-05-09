# Fut Worlds Pickem — task runner
# Run `just --list` to see all available recipes.

set dotenv-load := true

# Default: show available recipes
default:
    @just --list

# --- Local dev dependencies (Postgres + NATS + Jaeger) ------------------------

# Bring up the development dependencies
dev-up:
    docker compose -f infra/docker-compose.yml up -d

# Tear down the development dependencies
dev-down:
    docker compose -f infra/docker-compose.yml down

# Tail logs of all dev services
dev-logs:
    docker compose -f infra/docker-compose.yml logs -f

# --- Database migrations ------------------------------------------------------

# Run pending migrations against $DATABASE_URL
migrate:
    sqlx migrate run --source migrations

# Revert the latest migration
migrate-revert:
    sqlx migrate revert --source migrations

# Refresh the .sqlx offline cache (run after schema or query changes)
sqlx-prepare:
    cargo sqlx prepare --workspace -- --all-targets

# --- Rust services ------------------------------------------------------------

# Run the Telegram bot service
bot:
    cargo run -p bot

# Run the HTTP API service
api:
    cargo run -p api

# Run the events ingester / scheduler / scorer
events:
    cargo run -p events

# Operational CLI for the events service. Pass subcommands after the recipe,
# e.g. `just events-cli seed-tournament --competition WC --dry-run`
#       `just events-cli list-matches --competition WC`
#       `just events-cli list-teams --competition WC`
events-cli *args:
    cargo run -q -p events --bin events-cli -- {{args}}

# Run all three services in parallel (foreground; Ctrl+C stops all)
run-all:
    #!/usr/bin/env bash
    set -euo pipefail
    trap 'kill 0' EXIT
    cargo run -p bot &
    cargo run -p api &
    cargo run -p events &
    wait

# --- Frontend Mini App --------------------------------------------------------

# Install frontend dependencies
front-install:
    cd frontend/miniapp && pnpm install

# Run the Mini App dev server
front:
    cd frontend/miniapp && pnpm dev

# Build the Mini App for production
front-build:
    cd frontend/miniapp && pnpm build

# --- Quality gates ------------------------------------------------------------

# Run the full Rust test suite
test:
    cargo test --workspace

# Lint Rust code (clippy + rustfmt + sqlx offline cache)
lint:
    cargo clippy --workspace --all-targets -- -D warnings
    cargo fmt --all -- --check
    cargo sqlx prepare --workspace --check -- --all-targets

# Auto-format all Rust code
fmt:
    cargo fmt --all

# Type-check the workspace without building binaries
check:
    cargo check --workspace --all-targets

# Run every quality gate (used by CI mirrors)
ci: lint test
