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

    /// Cancel request was rejected by the broker.
    ///
    /// This can occur when a cancel races against an in-flight fill:
    /// the order fills before the cancel request reaches the broker,
    /// causing the broker to reject the cancel as the order is already complete.
    #[error("cancel rejected for order {order_id}: {reason}")]
    CancelReject { order_id: i32, reason: String },

    /// Connection lost during order execution with partial fill state.
    ///
    /// This error is raised when the TWS/Gateway connection is lost during
    /// order execution, potentially after a partial fill has occurred.
    /// The filled_quantity field captures the last known filled quantity
    /// before the disconnect, enabling reconciliation on reconnect.
    #[error("connection lost during order execution (order_id={order_id}, filled_quantity={filled_quantity})")]
    ConnectionLost { order_id: i32, filled_quantity: i64 },

    #[error("{0}")]
    Other(String),
}
