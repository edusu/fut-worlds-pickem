//! Cross-cutting utilities used by every service.

pub mod config;
pub mod errors;
pub mod tracing;

pub use config::Config;
pub use errors::{SharedError, SharedResult};
