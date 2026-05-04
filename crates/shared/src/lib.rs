//! Cross-cutting utilities used by every service.

pub mod config;
pub mod error;
pub mod tracing;

pub use config::Config;
pub use error::{report_to_anyhow, SharedError, SharedReport, SharedResult};
