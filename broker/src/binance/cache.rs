//! Local Binance order cache persistence.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use chrono::{DateTime, Utc};
use nanobook::Symbol;
use serde::{Deserialize, Serialize};

use super::CachedOrder;
use crate::error::BrokerError;
use crate::types::{BrokerSide, OrderId, OrderState};

#[derive(Debug, Clone, Default)]
pub struct BinanceOrderCache {
    pub orders: HashMap<OrderId, CachedOrder>,
}

impl BinanceOrderCache {
    pub fn new() -> Self {
        Self {
            orders: HashMap::new(),
        }
    }

    pub fn save_to_disk(&self, path: &Path) -> Result<(), BrokerError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                BrokerError::Other(format!(
                    "failed to create Binance order cache directory: {e}"
                ))
            })?;
        }
        let entries: Vec<CachedOrderRecord> = self
            .orders
            .iter()
            .map(|(order_id, order)| CachedOrderRecord::from_cached(*order_id, order))
            .collect();
        let json = serde_json::to_string_pretty(&entries).map_err(|e| {
            BrokerError::Other(format!("failed to serialize Binance order cache: {e}"))
        })?;
        fs::write(path, json)
            .map_err(|e| BrokerError::Other(format!("failed to write Binance order cache: {e}")))
    }

    pub fn load_from_disk(path: &Path) -> Result<Self, BrokerError> {
        if !path.exists() {
            return Ok(Self::new());
        }

        let json = fs::read_to_string(path)
            .map_err(|e| BrokerError::Other(format!("failed to read Binance order cache: {e}")))?;
        let entries: Vec<CachedOrderRecord> = serde_json::from_str(&json).map_err(|e| {
            BrokerError::Other(format!("failed to deserialize Binance order cache: {e}"))
        })?;
        let mut cache = Self::new();
        for entry in entries {
            let (order_id, order) = entry.into_cached()?;
            cache.orders.insert(order_id, order);
        }
        Ok(cache)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct CachedOrderRecord {
    order_id: u64,
    symbol: String,
    quantity: i64,
    side: String,
    status: String,
    binance_order_id: String,
    client_order_id: Option<String>,
    submitted_at: DateTime<Utc>,
}

impl CachedOrderRecord {
    fn from_cached(order_id: OrderId, order: &CachedOrder) -> Self {
        Self {
            order_id: order_id.0,
            symbol: order.symbol.as_str().to_string(),
            quantity: order.quantity,
            side: serialize_side(order.side).to_string(),
            status: serialize_status(order.status).to_string(),
            binance_order_id: order.binance_order_id.clone(),
            client_order_id: order.client_order_id.clone(),
            submitted_at: order.submitted_at,
        }
    }

    fn into_cached(self) -> Result<(OrderId, CachedOrder), BrokerError> {
        let symbol = Symbol::try_new(&self.symbol)
            .ok_or_else(|| BrokerError::InvalidSymbol(self.symbol.clone()))?;
        Ok((
            OrderId(self.order_id),
            CachedOrder {
                symbol,
                quantity: self.quantity,
                side: deserialize_side(&self.side)?,
                status: deserialize_status(&self.status)?,
                binance_order_id: self.binance_order_id,
                client_order_id: self.client_order_id,
                submitted_at: self.submitted_at,
            },
        ))
    }
}

fn serialize_side(side: BrokerSide) -> &'static str {
    match side {
        BrokerSide::Buy => "buy",
        BrokerSide::Sell => "sell",
    }
}

fn deserialize_side(side: &str) -> Result<BrokerSide, BrokerError> {
    match side {
        "buy" => Ok(BrokerSide::Buy),
        "sell" => Ok(BrokerSide::Sell),
        _ => Err(BrokerError::Other(format!(
            "invalid Binance cached order side: {side}"
        ))),
    }
}

fn serialize_status(status: OrderState) -> &'static str {
    match status {
        OrderState::Pending => "pending",
        OrderState::Submitted => "submitted",
        OrderState::PartiallyFilled => "partially_filled",
        OrderState::Filled => "filled",
        OrderState::Cancelled => "cancelled",
        OrderState::Rejected => "rejected",
    }
}

fn deserialize_status(status: &str) -> Result<OrderState, BrokerError> {
    match status {
        "pending" => Ok(OrderState::Pending),
        "submitted" => Ok(OrderState::Submitted),
        "partially_filled" => Ok(OrderState::PartiallyFilled),
        "filled" => Ok(OrderState::Filled),
        "cancelled" => Ok(OrderState::Cancelled),
        "rejected" => Ok(OrderState::Rejected),
        _ => Err(BrokerError::Other(format!(
            "invalid Binance cached order status: {status}"
        ))),
    }
}
