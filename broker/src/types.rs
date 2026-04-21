//! Shared broker types: positions, accounts, orders, quotes.

use std::fmt::Write;

use nanobook::{Price, Symbol};
use sha2::{Digest, Sha256};

use crate::error::BrokerError;

/// Broker-level position (the real-world counterpart, not the LOB position).
#[derive(Debug, Clone)]
pub struct Position {
    pub symbol: Symbol,
    /// Positive = long, negative = short.
    pub quantity: i64,
    pub avg_cost_cents: i64,
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
}

/// Account summary from the broker.
#[derive(Debug, Clone)]
pub struct Account {
    pub equity_cents: i64,
    pub buying_power_cents: i64,
    pub cash_cents: i64,
    pub gross_position_value_cents: i64,
}

/// Order to submit to a broker.
#[derive(Debug, Clone)]
pub struct BrokerOrder {
    pub symbol: Symbol,
    pub side: BrokerSide,
    pub quantity: u64,
    pub order_type: BrokerOrderType,
    pub client_order_id: Option<ClientOrderId>,
}

/// Buy or sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerSide {
    Buy,
    Sell,
}

/// Deterministic client-side order identifier.
///
/// The derived form is SHA-256 of a canonical `(scope, symbol, side, qty)`
/// tuple, hex-encoded and truncated to 32 chars. This fits Binance's
/// 36-character `newClientOrderId` limit and IBKR's 40-character `orderRef`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientOrderId(String);

impl ClientOrderId {
    pub fn derive(scope: &str, symbol: &str, side: BrokerSide, qty: u64) -> Self {
        let mut h = Sha256::new();
        h.update(scope.as_bytes());
        h.update(b"\0");
        h.update(symbol.as_bytes());
        h.update(b"\0");
        h.update(match side {
            BrokerSide::Buy => b"B",
            BrokerSide::Sell => b"S",
        });
        h.update(b"\0");
        h.update(qty.to_le_bytes());

        let digest = h.finalize();
        let mut hex = String::with_capacity(32);
        for b in &digest[..16] {
            write!(&mut hex, "{b:02x}").expect("writing to String cannot fail");
        }
        Self(hex)
    }

    pub fn new(value: impl Into<String>) -> Result<Self, BrokerError> {
        let value = value.into();
        if value.is_empty() || value.len() > 36 {
            return Err(BrokerError::Order(
                "client_order_id must be 1..=36 ASCII-safe characters".into(),
            ));
        }
        if !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'.' || b == b'-')
        {
            return Err(BrokerError::Order(
                "client_order_id contains unsafe query characters".into(),
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Market or limit order.
#[derive(Debug, Clone, Copy)]
pub enum BrokerOrderType {
    Market,
    Limit(Price),
}

/// Live quote from the broker.
#[derive(Debug, Clone)]
pub struct Quote {
    pub symbol: Symbol,
    pub bid_cents: i64,
    pub ask_cents: i64,
    pub last_cents: i64,
    pub volume: u64,
}

/// Last-seen bid/ask quote used to bound market-order fallbacks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BestQuote {
    pub bid_cents: i64,
    pub ask_cents: i64,
}

/// Opaque order ID returned by the broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

/// Status of a submitted order.
#[derive(Debug, Clone)]
pub struct BrokerOrderStatus {
    pub id: OrderId,
    pub status: OrderState,
    pub filled_quantity: u64,
    pub remaining_quantity: u64,
    pub avg_fill_price_cents: i64,
}

/// Lifecycle state of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderState {
    Pending,
    Submitted,
    PartiallyFilled,
    Filled,
    Cancelled,
    Rejected,
}
