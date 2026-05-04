//! Pure domain layer for the FutWorldsPickem system.
//!
//! This crate contains only data types and abstract behavior. It must remain
//! free of I/O so it can be reused across services without dragging in
//! database drivers, network clients, or runtime dependencies.

pub mod models;
pub mod repository;
pub mod scoring;

pub use models::*;
