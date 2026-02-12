//! nanobook-rebalancer: Portfolio rebalancer bridging nanobook to Interactive Brokers.
//!
//! Reads target weights from a JSON file, connects to IBKR for live positions
//! and prices, computes the diff, and executes limit orders with risk checks
//! and an audit trail.

pub mod audit;
pub mod broker;
pub mod config;
pub mod diff;
pub mod error;
pub mod execution;
pub mod reconcile;
pub mod risk;
pub mod target;
