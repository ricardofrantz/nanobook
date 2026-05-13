//! Broker trait and implementations for nanobook.
//!
//! Provides a generic `Broker` trait that abstracts over different brokerages.
//! Implementations:
//!
//! - **IBKR** (feature `ibkr`): Interactive Brokers via TWS API
//! - **Binance** (feature `binance`): Binance spot REST API

pub mod error;
pub mod mock;
pub mod types;

/// Shared parsing helpers for broker REST responses. Compiled only
/// when at least one HTTP-based adapter is active (IBKR's ibapi is
/// binary, but its account-summary strings share the same concerns).
#[cfg(any(feature = "binance", feature = "ibkr"))]
pub(crate) mod parse;

#[cfg(feature = "ibkr")]
pub mod ibkr;

#[cfg(feature = "binance")]
pub mod binance;

pub use error::BrokerError;
pub use types::*;

use nanobook::Symbol;

/// A broker connection that can fetch positions, submit orders, and get quotes.
///
/// ```
/// use std::time::SystemTime;
///
/// use nanobook::{Price, Symbol};
/// use nanobook_broker::{
///     Account, Broker, BrokerError, BrokerOrder, BrokerOrderStatus, BrokerOrderType,
///     BrokerSide, OrderId, OrderState, Position, Quote,
/// };
///
/// struct PaperBroker;
///
/// impl Broker for PaperBroker {
///     fn connect(&mut self) -> Result<(), BrokerError> { Ok(()) }
///     fn disconnect(&mut self) -> Result<(), BrokerError> { Ok(()) }
///     fn positions(&self) -> Result<Vec<Position>, BrokerError> { Ok(Vec::new()) }
///
///     fn account(&self) -> Result<Account, BrokerError> {
///         Ok(Account {
///             equity_cents: 100_000_00,
///             buying_power_cents: 100_000_00,
///             cash_cents: 100_000_00,
///             gross_position_value_cents: 0,
///         })
///     }
///
///     fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
///         assert_eq!(order.symbol, Symbol::new("AAPL"));
///         Ok(OrderId(1))
///     }
///
///     fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError> {
///         Ok(BrokerOrderStatus {
///             id,
///             status: OrderState::Submitted,
///             filled_quantity: 0,
///             remaining_quantity: 10,
///             avg_fill_price_cents: 0,
///         })
///     }
///
///     fn open_orders(&self) -> Result<Vec<BrokerOrderStatus>, BrokerError> { Ok(Vec::new()) }
///     fn cancel_order(&self, _id: OrderId) -> Result<(), BrokerError> { Ok(()) }
///
///     fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
///         Ok(Quote {
///             symbol: *symbol,
///             bid_cents: 150_00,
///             ask_cents: 150_05,
///             last_cents: 150_02,
///             volume: 1_000,
///             timestamp: SystemTime::now(),
///         })
///     }
/// }
///
/// let mut broker = PaperBroker;
/// broker.connect().unwrap();
/// let quote = broker.quote(&Symbol::new("AAPL")).unwrap();
/// let order_id = broker.submit_order(&BrokerOrder {
///     symbol: quote.symbol,
///     side: BrokerSide::Buy,
///     quantity: 10,
///     order_type: BrokerOrderType::Limit(Price(quote.ask_cents)),
///     client_order_id: None,
/// }).unwrap();
///
/// assert_eq!(broker.order_status(order_id).unwrap().status, OrderState::Submitted);
/// ```
pub trait Broker {
    /// Connect to the broker.
    fn connect(&mut self) -> Result<(), BrokerError>;

    /// Disconnect gracefully.
    fn disconnect(&mut self) -> Result<(), BrokerError>;

    /// Get all current positions.
    fn positions(&self) -> Result<Vec<Position>, BrokerError>;

    /// Get account summary (equity, buying power, etc.).
    fn account(&self) -> Result<Account, BrokerError>;

    /// Submit an order. Returns order ID.
    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError>;

    /// Get status of a submitted order.
    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError>;

    /// Get all open orders from the broker.
    fn open_orders(&self) -> Result<Vec<BrokerOrderStatus>, BrokerError>;

    /// Cancel a pending order.
    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError>;

    /// Get current quote for a symbol.
    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError>;
}
