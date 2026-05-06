//! Events service library — shared between the long-running daemon
//! (`bin/events`) and the operational CLI (`bin/events-cli`).
//!
//! The library exposes the ingester / scheduler / scorer modules so any
//! binary inside the crate can compose them; the daemon wires all three
//! into a single `tokio::try_join!`, while the CLI reuses just the
//! ingester adapter for one-shot tournament-data seeds.

pub mod ingester;
pub mod scheduler;
pub mod scorer;
