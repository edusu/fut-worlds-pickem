//! NATS-based messaging contract between services.
//!
//! Every cross-service signal goes through this crate. Adding a new event
//! means: (1) adding a versioned variant to the right enum in `events`,
//! (2) declaring a topic constant in `topics`, (3) using `Publisher` /
//! `Subscriber` to wire it into producer and consumer services.

pub mod events;
pub mod publisher;
pub mod subscriber;
pub mod topics;

pub use events::*;
pub use publisher::Publisher;
pub use subscriber::Subscriber;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum MessagingError {
    #[error("nats error: {0}")]
    Nats(String),
    #[error("serialization error: {0}")]
    Serde(#[from] serde_json::Error),
}

pub type MessagingResult<T> = Result<T, MessagingError>;
