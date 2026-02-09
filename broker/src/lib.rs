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

#[cfg(feature = "ibkr")]
pub mod ibkr;

#[cfg(feature = "binance")]
pub mod binance;

pub use error::BrokerError;
pub use types::*;

use nanobook::Symbol;

/// A broker connection that can fetch positions, submit orders, and get quotes.
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

    /// Cancel a pending order.
    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError>;

    /// Get current quote for a symbol.
    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError>;
}
