//! Cross-cutting utilities used by every service.

pub mod config;
pub mod error;
pub mod flags;
pub mod tracing;

pub use config::Config;
pub use error::{report_to_anyhow, SharedError, SharedReport, SharedResult};
pub use flags::flag_emoji;

/// Upstream competition code for the v1 tournament. football-data.org uses
/// `"WC"` for the FIFA World Cup. Hardcoded because v1 only targets the
/// World Cup; promoting this to config is a low-cost follow-up the day a
/// second tournament shows up. Both the events bootstrap and the api
/// `routes/parents` resolve the tournament UUID from this constant.
pub const V1_TOURNAMENT_EXTERNAL_ID: &str = "WC";
