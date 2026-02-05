// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! # limit-order-book
//!
//! A deterministic limit order book and matching engine for testing trading algorithms.
//!
//! ## Features
//!
//! - **Order types**: Limit, Market, Cancel, Modify
//! - **Time-in-force**: GTC (Good-til-cancelled), IOC (Immediate-or-cancel), FOK (Fill-or-kill)
//! - **Price-time priority**: FIFO matching at each price level
//! - **Deterministic replay**: Record events and replay to reconstruct exact state
//! - **Fixed-point prices**: Avoid floating-point errors with integer cents
//!
//! ## Quick Start
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//!
//! // Place some resting asks (sell orders)
//! exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
//! exchange.submit_limit(Side::Sell, Price(102_00), 200, TimeInForce::GTC);
//!
//! // Place a bid that crosses — this will match!
//! let result = exchange.submit_limit(Side::Buy, Price(101_00), 50, TimeInForce::GTC);
//!
//! assert_eq!(result.filled_quantity, 50);
//! assert_eq!(result.trades.len(), 1);
//! assert_eq!(result.trades[0].price, Price(101_00));
//! ```
//!
//! ## Price Representation
//!
//! Prices are stored as [`i64`] in the smallest unit (e.g., cents):
//!
//! ```
//! use limit_order_book::Price;
//!
//! let price = Price(100_50);  // $100.50
//! assert_eq!(format!("{}", price), "$100.50");
//! ```
//!
//! ## Time-in-Force
//!
//! | TIF | Behavior |
//! |-----|----------|
//! | **GTC** | Rests on book until filled or cancelled |
//! | **IOC** | Fill immediately, cancel unfilled remainder |
//! | **FOK** | Fill entirely or cancel entirely (no partial fills) |
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//!
//! // IOC: Fill what's available, cancel the rest
//! exchange.submit_limit(Side::Sell, Price(100_00), 30, TimeInForce::GTC);
//! let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);
//! assert_eq!(result.filled_quantity, 30);
//! assert_eq!(result.cancelled_quantity, 70);
//!
//! // FOK: Must fill entirely or nothing happens
//! exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
//! let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);
//! assert_eq!(result.filled_quantity, 0);  // Rejected: only 50 available
//! assert!(result.trades.is_empty());
//! ```
//!
//! ## Market Orders
//!
//! Market orders execute at the best available prices:
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//! exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
//! exchange.submit_limit(Side::Sell, Price(101_00), 50, TimeInForce::GTC);
//!
//! // Market buy sweeps through price levels
//! let result = exchange.submit_market(Side::Buy, 75);
//! assert_eq!(result.trades.len(), 2);
//! assert_eq!(result.trades[0].price, Price(100_00));  // Best price first
//! assert_eq!(result.trades[1].price, Price(101_00));
//! ```
//!
//! ## Cancel and Modify
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//!
//! let order = exchange.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
//!
//! // Cancel: removes the order from the book
//! let cancel = exchange.cancel(order.order_id);
//! assert!(cancel.success);
//!
//! // Modify: cancel-and-replace (new order gets new ID, loses time priority)
//! let order2 = exchange.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
//! let modify = exchange.modify(order2.order_id, Price(98_00), 150);
//! assert!(modify.success);
//! assert_ne!(modify.new_order_id, Some(order2.order_id));
//! ```
//!
//! ## Event Replay
//!
//! All operations are recorded as events for deterministic replay:
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//! exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
//! exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
//! exchange.submit_limit(Side::Buy, Price(101_00), 50, TimeInForce::GTC);
//!
//! // Save events
//! let events = exchange.events().to_vec();
//!
//! // Replay on a fresh exchange — produces identical state
//! let replayed = Exchange::replay(&events);
//! assert_eq!(exchange.best_bid_ask(), replayed.best_bid_ask());
//! assert_eq!(exchange.trades().len(), replayed.trades().len());
//! ```
//!
//! ## Book Snapshots
//!
//! Get market data snapshots:
//!
//! ```
//! use limit_order_book::{Exchange, Side, Price, TimeInForce};
//!
//! let mut exchange = Exchange::new();
//! exchange.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
//! exchange.submit_limit(Side::Buy, Price(100_00), 200, TimeInForce::GTC);
//! exchange.submit_limit(Side::Sell, Price(101_00), 150, TimeInForce::GTC);
//!
//! let snap = exchange.depth(10);  // Top 10 levels each side
//!
//! assert_eq!(snap.best_bid(), Some(Price(100_00)));
//! assert_eq!(snap.best_ask(), Some(Price(101_00)));
//! assert_eq!(snap.spread(), Some(100));  // $1.00
//! ```

mod book;
mod event;
mod exchange;
mod level;
mod matching;
mod order;
mod price_levels;
mod result;
mod side;
mod snapshot;
mod tif;
mod trade;
mod types;

// Re-export public API
pub use book::OrderBook;
pub use event::{ApplyResult, Event};
pub use exchange::Exchange;
pub use level::Level;
pub use matching::MatchResult;
pub use order::{Order, OrderStatus};
pub use price_levels::PriceLevels;
pub use result::{CancelError, CancelResult, ModifyError, ModifyResult, SubmitResult};
pub use side::Side;
pub use snapshot::{BookSnapshot, LevelSnapshot};
pub use tif::TimeInForce;
pub use trade::Trade;
pub use types::{OrderId, Price, Quantity, Timestamp, TradeId};
