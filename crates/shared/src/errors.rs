use thiserror::Error;

/// Cross-cutting error type. Service-specific errors should wrap this rather
/// than inherit from it.
#[derive(Debug, Error)]
pub enum SharedError {
    #[error("missing required config value: {0}")]
    MissingConfig(&'static str),
    #[error("invalid config value for {key}: {reason}")]
    InvalidConfig { key: &'static str, reason: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

pub type SharedResult<T> = Result<T, SharedError>;
