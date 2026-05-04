# syntax=docker/dockerfile:1.6
# Multi-stage build for the bot service.
# Layer 1: cargo-chef plan         — captures the dependency graph.
# Layer 2: cargo-chef cook          — compiles deps only (cacheable).
# Layer 3: build the bin            — recompiles only the code that changed.
# Layer 4: slim runtime              — debian-slim with the static-ish binary.

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json -p bot
COPY . .
RUN cargo build --release -p bot

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY --from=builder /app/target/release/bot /usr/local/bin/bot
USER 1000
ENTRYPOINT ["/usr/local/bin/bot"]
