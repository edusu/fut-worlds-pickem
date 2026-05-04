//! NATS-based messaging contract between services.
//!
//! Every cross-service signal goes through this crate. Adding a new event
//! means: (1) adding a versioned variant to the right enum in `events`,
//! (2) declaring a topic constant in `topics`, (3) using `Publisher` /
//! `Subscriber` to wire it into producer and consumer services.

pub mod error;
pub mod events;
pub mod publisher;
pub mod subscriber;
pub mod topics;

pub use error::{MessagingError, MessagingReport, MessagingResult};
pub use events::*;
pub use publisher::Publisher;
pub use subscriber::Subscriber;
