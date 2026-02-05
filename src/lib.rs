//! # limit-order-book
//!
//! A deterministic, nanosecond-precision limit order book and matching engine
//! for testing trading algorithms.
//!
//! ## Features
//!
//! - **Order types**: Limit, Market, Cancel, Modify
//! - **Time-in-force**: GTC, IOC, FOK
//! - **Price-time priority**: FIFO matching at each price level
//! - **Deterministic**: Same inputs → same outputs
//!
//! ## Quick Example
//!
//! ```
//! use limit_order_book::{Side, Price, TimeInForce};
//!
//! // Types are ready to use
//! let price = Price(100_50); // $100.50
//! let side = Side::Buy;
//! let tif = TimeInForce::GTC;
//!
//! assert_eq!(side.opposite(), Side::Sell);
//! assert!(tif.can_rest());
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
