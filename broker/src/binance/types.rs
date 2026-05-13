//! Binance-specific API response types.

use serde::Deserialize;
use nanobook::Symbol;
use crate::types::{OrderId, OrderState};

/// Binance account balance entry.
#[derive(Debug, Deserialize)]
pub struct BalanceInfo {
    pub asset: String,
    pub free: String,
    pub locked: String,
}

/// Binance position information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PositionInfo {
    pub symbol: String,
    pub position_amt: String,
    #[serde(default)]
    pub entry_price: String,
}

/// Binance order information.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderInfo {
    pub symbol: String,
    pub order_id: u64,
    pub status: String,
    pub side: String,
    #[serde(default)]
    pub orig_qty: String,
    #[serde(default)]
    pub executed_qty: String,
}

/// Binance account info response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub balances: Vec<BalanceInfo>,
    #[serde(default)]
    pub positions: Vec<PositionInfo>,
    #[serde(default)]
    pub open_orders: Vec<OrderInfo>,
    #[serde(default)]
    pub can_trade: bool,
}

/// Discrepancy report from state reconciliation.
#[derive(Debug, Clone)]
pub struct DiscrepancyReport {
    pub discrepancies: Vec<Discrepancy>,
    pub has_critical_issues: bool,
}

/// Types of discrepancies detected during reconciliation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discrepancy {
    /// Order exists on broker but not in local cache.
    OrphanOrder {
        order_id: OrderId,
    },
    /// Order exists in local cache but not on broker.
    MissingOrder {
        order_id: OrderId,
    },
    /// Order status differs between local cache and broker.
    OrderStatusMismatch {
        order_id: OrderId,
        local_status: String,
        broker_status: OrderState,
    },
    /// Position quantity differs between local cache and broker.
    PositionMismatch {
        symbol: Symbol,
        local_quantity: i64,
        broker_quantity: i64,
    },
}

/// Binance order response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OrderResponse {
    pub symbol: String,
    pub order_id: u64,
    pub status: String,
    pub executed_qty: String,
    #[serde(default)]
    pub cummulative_quote_qty: String,
}

/// Binance ticker response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BookTicker {
    pub symbol: String,
    pub bid_price: String,
    pub bid_qty: String,
    pub ask_price: String,
    pub ask_qty: String,
}
