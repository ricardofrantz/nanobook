//! Mock Binance implementation with failure injection support.
//!
//! This module provides a mock of the Binance spot API that can inject
//! specific failure modes for testing broker resilience and idempotency.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use nanobook::Symbol;

use nanobook_broker::Broker;
use nanobook_broker::error::BrokerError;
use nanobook_broker::types::*;

#[cfg(feature = "binance")]
use nanobook_broker::binance::types::{AccountInfo, BalanceInfo, OrderInfo};
#[cfg(feature = "binance")]
use nanobook_broker::binance::{
    check_audit_log_for_sequence, log_idempotency_rejection, log_order_submitted,
};

/// Failure modes that can be injected by the mock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureMode {
    /// Network timeout during order submission
    NetworkTimeout,
    /// Rate limit exceeded (429 error)
    RateLimitExceeded,
    /// Invalid symbol error
    InvalidSymbol,
    /// Insufficient funds error
    InsufficientFunds,
    /// Duplicate order ID error
    DuplicateOrder,
    /// Internal server error (500)
    ServerError,
}

/// Mock order stored in the MockBinance.
#[derive(Debug, Clone)]
pub struct MockOrder {
    pub symbol: String,
    pub quantity: String,
    pub side: String,
    pub status: OrderState,
    pub client_order_id: Option<String>,
    pub price: Option<String>,
}

/// Mock Binance API with deterministic failure injection.
///
/// This mock simulates Binance API behavior, including client order ID
/// deduplication (critical for idempotency testing), order lifecycle
/// management, and failure injection.
pub struct MockBinance {
    orders: Mutex<HashMap<String, MockOrder>>,
    client_order_ids: Mutex<HashSet<String>>,
    next_order_id: AtomicU64,
    active_failure: Mutex<Option<FailureMode>>,
    websocket_disconnected: Mutex<bool>,
}

impl MockBinance {
    /// Create a new MockBinance instance.
    pub fn new() -> Self {
        Self {
            orders: Mutex::new(HashMap::new()),
            client_order_ids: Mutex::new(HashSet::new()),
            next_order_id: AtomicU64::new(1),
            active_failure: Mutex::new(None),
            websocket_disconnected: Mutex::new(false),
        }
    }

    /// Submit an order to the mock Binance.
    ///
    /// Returns the order ID as a string on success.
    /// Returns an error string on failure (including duplicate client_order_id).
    pub fn submit_order(
        &self,
        symbol: &str,
        side: &str,
        qty: &str,
        client_order_id: Option<&str>,
    ) -> Result<String, String> {
        // Check for active failure injection
        if let Some(failure) = *self.active_failure.lock().unwrap() {
            return Err(match failure {
                FailureMode::NetworkTimeout => "Network timeout".to_string(),
                FailureMode::RateLimitExceeded => "Rate limit exceeded".to_string(),
                FailureMode::InvalidSymbol => "Invalid symbol".to_string(),
                FailureMode::InsufficientFunds => "Insufficient funds".to_string(),
                FailureMode::DuplicateOrder => "Duplicate order".to_string(),
                FailureMode::ServerError => "Internal server error".to_string(),
            });
        }

        // Check for duplicate client_order_id
        if let Some(cid) = client_order_id {
            let client_ids = self.client_order_ids.lock().unwrap();
            if client_ids.contains(cid) {
                return Err(format!("Duplicate client_order_id: {}", cid));
            }
        }

        // Generate order ID
        let order_id = self
            .next_order_id
            .fetch_add(1, Ordering::Relaxed)
            .to_string();

        // Store client_order_id if provided
        if let Some(cid) = client_order_id {
            self.client_order_ids
                .lock()
                .unwrap()
                .insert(cid.to_string());
        }

        // Create and store order
        let order = MockOrder {
            symbol: symbol.to_string(),
            quantity: qty.to_string(),
            side: side.to_string(),
            status: OrderState::Submitted,
            client_order_id: client_order_id.map(|s| s.to_string()),
            price: None,
        };

        self.orders.lock().unwrap().insert(order_id.clone(), order);

        Ok(order_id)
    }

    /// Get an order by ID.
    pub fn get_order(&self, order_id: &str) -> Option<MockOrder> {
        self.orders.lock().unwrap().get(order_id).cloned()
    }

    /// Get all orders.
    pub fn all_orders(&self) -> Vec<MockOrder> {
        self.orders.lock().unwrap().values().cloned().collect()
    }

    /// Simulate a WebSocket disconnect while preserving exchange-side state.
    pub fn simulate_websocket_disconnect(&self) {
        *self.websocket_disconnected.lock().unwrap() = true;
    }

    /// Simulate a WebSocket reconnect while preserving exchange-side state.
    pub fn simulate_websocket_reconnect(&self) {
        *self.websocket_disconnected.lock().unwrap() = false;
    }

    /// Return whether the simulated WebSocket is currently disconnected.
    pub fn is_websocket_disconnected(&self) -> bool {
        *self.websocket_disconnected.lock().unwrap()
    }

    /// Mark an order as partially filled.
    pub fn simulate_partial_fill(&self, order_id: &str) -> Result<(), String> {
        let mut orders = self.orders.lock().unwrap();
        if let Some(order) = orders.get_mut(order_id) {
            order.status = OrderState::PartiallyFilled;
            Ok(())
        } else {
            Err(format!("Order {} not found", order_id))
        }
    }

    /// Cancel an order by ID.
    pub fn cancel_order(&self, order_id: &str) -> Result<(), String> {
        let mut orders = self.orders.lock().unwrap();
        if let Some(order) = orders.get_mut(order_id) {
            order.status = OrderState::Cancelled;
            Ok(())
        } else {
            Err(format!("Order {} not found", order_id))
        }
    }

    /// Get all open orders (not filled, cancelled, or rejected).
    pub fn get_open_orders(&self) -> Vec<MockOrder> {
        self.orders
            .lock()
            .unwrap()
            .values()
            .filter(|o| {
                matches!(
                    o.status,
                    OrderState::Pending | OrderState::Submitted | OrderState::PartiallyFilled
                )
            })
            .cloned()
            .collect()
    }

    /// Get account info (for reconciliation testing).
    #[cfg(feature = "binance")]
    pub fn account_info(&self) -> AccountInfo {
        let orders = self.orders.lock().unwrap();
        let open_orders: Vec<OrderInfo> = orders
            .iter()
            .filter(|(_, o)| {
                matches!(
                    o.status,
                    OrderState::Pending | OrderState::Submitted | OrderState::PartiallyFilled
                )
            })
            .map(|(id, o)| OrderInfo {
                symbol: o.symbol.clone(),
                order_id: id.parse().unwrap_or(0),
                status: match o.status {
                    OrderState::Submitted => "NEW".to_string(),
                    OrderState::PartiallyFilled => "PARTIALLY_FILLED".to_string(),
                    OrderState::Filled => "FILLED".to_string(),
                    OrderState::Cancelled => "CANCELED".to_string(),
                    OrderState::Rejected => "REJECTED".to_string(),
                    _ => "NEW".to_string(),
                },
                side: o.side.clone(),
                orig_qty: o.quantity.clone(),
                executed_qty: match o.status {
                    OrderState::Filled => o.quantity.clone(),
                    OrderState::PartiallyFilled => {
                        let qty: u64 = o.quantity.parse().unwrap_or(0);
                        (qty / 2).to_string()
                    }
                    _ => "0".to_string(),
                },
            })
            .collect();

        AccountInfo {
            balances: vec![BalanceInfo {
                asset: "USDT".to_string(),
                free: "10000.0".to_string(),
                locked: "0.0".to_string(),
            }],
            positions: vec![],
            open_orders,
            can_trade: true,
        }
    }

    /// Reset all state (clear orders and client_order_ids).
    pub fn reset(&self) {
        self.orders.lock().unwrap().clear();
        self.client_order_ids.lock().unwrap().clear();
        self.next_order_id.store(1, Ordering::Relaxed);
        *self.websocket_disconnected.lock().unwrap() = false;
    }

    /// Inject a failure mode.
    pub fn inject_failure(&self, mode: FailureMode) {
        *self.active_failure.lock().unwrap() = Some(mode);
    }

    /// Clear any active failure injection.
    pub fn clear_failure(&self) {
        *self.active_failure.lock().unwrap() = None;
    }

    /// Get the next order ID (for testing).
    pub fn next_order_id(&self) -> u64 {
        self.next_order_id.load(Ordering::Relaxed)
    }
}

impl Default for MockBinance {
    fn default() -> Self {
        Self::new()
    }
}

/// MockBroker that wraps MockBinance and implements the Broker trait.
pub struct MockBroker {
    connected: bool,
    binance: MockBinance,
    mock_positions: Vec<Position>,
    mock_account: Account,
    mock_quotes: HashMap<Symbol, Quote>,
    /// Optional path to audit log file for idempotency tracking.
    audit_log_path: Option<PathBuf>,
}

impl MockBroker {
    /// Create a new MockBroker with default mock data.
    pub fn new() -> Self {
        Self {
            connected: false,
            binance: MockBinance::new(),
            mock_positions: Vec::new(),
            mock_account: Account {
                equity_cents: 100_000_000,
                buying_power_cents: 100_000_000,
                cash_cents: 100_000_000,
                gross_position_value_cents: 0,
            },
            mock_quotes: HashMap::new(),
            audit_log_path: None,
        }
    }

    /// Set mock positions.
    pub fn with_positions(mut self, positions: Vec<Position>) -> Self {
        self.mock_positions = positions;
        self
    }

    /// Set mock account.
    pub fn with_account(mut self, account: Account) -> Self {
        self.mock_account = account;
        self
    }

    /// Set mock quote for a symbol.
    pub fn with_quote(mut self, symbol: Symbol, quote: Quote) -> Self {
        self.mock_quotes.insert(symbol, quote);
        self
    }

    /// Set the audit log path for idempotency tracking.
    pub fn with_audit_log_path(mut self, path: PathBuf) -> Self {
        self.audit_log_path = Some(path);
        self
    }

    /// Get the underlying MockBinance for direct access.
    pub fn binance(&self) -> &MockBinance {
        &self.binance
    }
}

impl Default for MockBroker {
    fn default() -> Self {
        Self::new()
    }
}

impl Broker for MockBroker {
    fn connect(&mut self) -> Result<(), BrokerError> {
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), BrokerError> {
        self.connected = false;
        Ok(())
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        Ok(self.mock_positions.clone())
    }

    fn account(&self) -> Result<Account, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        Ok(self.mock_account.clone())
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        let side_str = match order.side {
            BrokerSide::Buy => "BUY",
            BrokerSide::Sell => "SELL",
        };

        let qty_str = order.quantity.to_string();
        let client_order_id = order.client_order_id.as_ref().map(|cid| cid.as_str());

        // Extract sequence number from client_order_id if it follows the pattern
        // Format: "nanobook-{short_uuid}-{sequence}"
        let sequence_number = client_order_id
            .and_then(|cid| cid.rsplit('-').next().and_then(|s| s.parse::<u64>().ok()));

        // Check for duplicate in audit log if enabled (requires binance feature)
        #[cfg(feature = "binance")]
        if let (Some(audit_path), Some(seq)) = (&self.audit_log_path, sequence_number) {
            if check_audit_log_for_sequence(audit_path, seq).unwrap_or(false) {
                let cid_str = client_order_id.unwrap_or("");
                log_idempotency_rejection(
                    audit_path,
                    order.symbol,
                    seq,
                    cid_str,
                    "duplicate sequence number in audit log",
                );
                return Err(BrokerError::DuplicateOrder {
                    client_order_id: cid_str.to_string(),
                });
            }
        }

        // Submit the order
        let order_id = self
            .binance
            .submit_order(order.symbol.as_str(), side_str, &qty_str, client_order_id)
            .map_err(|e| BrokerError::Order(e))?
            .parse::<u64>()
            .map_err(|e| BrokerError::Order(format!("Invalid order ID: {}", e)))?;

        // Log order submission to audit log if enabled (requires binance feature)
        #[cfg(feature = "binance")]
        if let (Some(audit_path), Some(seq), Some(cid)) =
            (&self.audit_log_path, sequence_number, client_order_id)
        {
            log_order_submitted(audit_path, OrderId(order_id), order.symbol, seq, cid);
        }

        Ok(OrderId(order_id))
    }

    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        let order_id_str = id.0.to_string();
        let order = self
            .binance
            .get_order(&order_id_str)
            .ok_or_else(|| BrokerError::Order(format!("Order {} not found", order_id_str)))?;

        // Parse quantity from string
        let quantity: u64 = order
            .quantity
            .parse()
            .map_err(|e| BrokerError::Order(format!("Invalid quantity: {}", e)))?;

        Ok(BrokerOrderStatus {
            id,
            status: order.status,
            filled_quantity: match order.status {
                OrderState::Filled => quantity,
                OrderState::PartiallyFilled => quantity / 2,
                _ => 0,
            },
            remaining_quantity: match order.status {
                OrderState::Filled => 0,
                OrderState::PartiallyFilled => quantity / 2,
                _ => quantity,
            },
            avg_fill_price_cents: 0,
        })
    }

    fn open_orders(&self) -> Result<Vec<BrokerOrderStatus>, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        let orders = self.binance.get_open_orders();
        let mut result = Vec::new();

        for order in orders {
            let order_id: u64 = order.symbol.parse().unwrap_or_else(|_| 0); // Fallback, though this shouldn't happen
            let quantity: u64 = order
                .quantity
                .parse()
                .map_err(|e| BrokerError::Order(format!("Invalid quantity: {}", e)))?;

            result.push(BrokerOrderStatus {
                id: OrderId(order_id),
                status: order.status,
                filled_quantity: 0,
                remaining_quantity: quantity,
                avg_fill_price_cents: 0,
            });
        }

        Ok(result)
    }

    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        let order_id_str = id.0.to_string();
        self.binance
            .cancel_order(&order_id_str)
            .map_err(|e| BrokerError::Order(e))
    }

    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        self.mock_quotes
            .get(symbol)
            .cloned()
            .ok_or_else(|| BrokerError::InvalidSymbol(symbol.as_str().to_string()))
    }
}
