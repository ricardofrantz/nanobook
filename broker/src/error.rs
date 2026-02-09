//! Broker error types.

/// Errors that can occur during broker operations.
#[derive(Debug, thiserror::Error)]
pub enum BrokerError {
    #[error("connection error: {0}")]
    Connection(String),

    #[error("order error: {0}")]
    Order(String),

    #[error("not connected")]
    NotConnected,

    #[error("invalid symbol: {0}")]
    InvalidSymbol(String),

    #[error("authentication error: {0}")]
    Auth(String),

    #[error("rate limit exceeded")]
    RateLimit,

    #[error("{0}")]
    Other(String),
}
