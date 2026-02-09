//! Shared broker types: positions, accounts, orders, quotes.

use nanobook::{Price, Symbol};

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
}

/// Buy or sell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrokerSide {
    Buy,
    Sell,
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
