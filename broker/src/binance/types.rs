//! Binance-specific API response types.

use serde::Deserialize;

/// Binance account balance entry.
#[derive(Debug, Deserialize)]
pub struct BalanceInfo {
    pub asset: String,
    pub free: String,
    pub locked: String,
}

/// Binance account info response.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountInfo {
    pub balances: Vec<BalanceInfo>,
    #[serde(default)]
    pub can_trade: bool,
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
