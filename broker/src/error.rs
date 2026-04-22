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

    #[error("market order rejected: no NBBO quote available for {symbol}")]
    NoQuoteForMarketOrder { symbol: String },

    #[error("market orders are disabled (strict-market-reject feature)")]
    MarketOrderRejected,

    /// An upstream broker field contained `NaN`, `+Inf`, or `-Inf`.
    /// Raising this explicitly stops the silent `NaN as i64 → 0` and
    /// `Inf as i64 → i64::MAX/MIN` saturation paths.
    #[error("{field} received non-finite value: {value}")]
    NonFiniteValue { field: &'static str, value: f64 },

    /// A fixed-point-scaled broker value does not fit in `i64`.
    /// Example: an upstream field reporting `1e20` dollars would scale
    /// to `1e22` cents, well beyond `i64::MAX ≈ 9.22e18`.
    #[error("{field} out of i64 range after scaling: {value}")]
    ValueOutOfRange { field: &'static str, value: f64 },

    #[error("{0}")]
    Other(String),
}
