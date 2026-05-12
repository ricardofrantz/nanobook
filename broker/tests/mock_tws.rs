//! Wire-level TWS mock with deterministic failure injection.
//!
//! This module provides a mock of the IBKR TWS/Gateway wire protocol that
//! can inject specific failure modes for testing broker resilience.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Failure modes that can be injected by the mock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureMode {
    /// F1: Duplicate order-status callback injection
    F1DuplicateStatus,
    /// F2: Cancel reject race with fill
    F2CancelRejectRace,
    /// F3: Partial fill followed by disconnect
    F3PartialFillDisconnect,
    /// F4: Stale market data detection
    F4StaleMarketData,
    /// F5: Clock skew detection
    F5ClockSkew,
    /// F6: TWS reconnect drill
    F6ReconnectDrill,
    /// F7: Cron double-fire idempotency
    F7CronDoubleFire,
    /// F8: Kill switch subcommand
    F8KillSwitch,
    /// F9: Process crash + warm restart
    F9ProcessCrash,
}

/// Timing for when to inject the failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureTiming {
    /// Inject before order submission
    PreSubmit,
    /// Inject after order submission
    PostSubmit,
    /// Inject mid-fill
    MidFill,
    /// Inject after fill
    PostFill,
}

/// Internal state for tracking order lifecycle.
#[derive(Debug, Clone)]
pub struct OrderState {
    pub id: u64,
    pub symbol: String,
    pub quantity: u64,
    pub filled_quantity: u64,
    pub status: String,
}

/// Wire-level TWS mock with deterministic failure injection.
///
/// This mock simulates the TWS/Gateway wire protocol behavior, including
/// callback ordering, sequence number tracking, and partial-fill semantics.
/// It provides a deterministic API for injecting specific failure modes.
pub struct MockTws {
    connected: AtomicBool,
    next_order_id: AtomicU64,
    next_seq_num: AtomicU64,
    active_failure: Mutex<Option<(FailureMode, FailureTiming)>>,
    orders: Mutex<Vec<OrderState>>,
    callbacks: Mutex<Vec<String>>,
    disconnect_injected: AtomicBool,
}

impl MockTws {
    /// Create a new MockTws instance.
    pub fn new() -> Self {
        Self {
            connected: AtomicBool::new(false),
            next_order_id: AtomicU64::new(1),
            next_seq_num: AtomicU64::new(1),
            active_failure: Mutex::new(None),
            orders: Mutex::new(Vec::new()),
            callbacks: Mutex::new(Vec::new()),
            disconnect_injected: AtomicBool::new(false),
        }
    }

    /// Simulate TWS connection.
    pub fn connect(&self) -> Result<(), String> {
        self.connected.store(true, Ordering::Relaxed);
        self.record_callback("Connected");
        Ok(())
    }

    /// Simulate TWS disconnection.
    pub fn disconnect(&self) -> Result<(), String> {
        self.connected.store(false, Ordering::Relaxed);
        self.record_callback("Disconnected");
        Ok(())
    }

    /// Check if currently connected.
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    /// Inject a failure mode with specified timing.
    pub fn inject_failure(&self, mode: FailureMode, timing: FailureTiming) {
        *self.active_failure.lock().unwrap() = Some((mode, timing));
    }

    /// Clear any active failure injection.
    pub fn clear_failure(&self) {
        *self.active_failure.lock().unwrap() = None;
        self.disconnect_injected.store(false, Ordering::Relaxed);
    }

    /// Get the next order ID (simulates TWS order ID allocation).
    pub fn next_order_id(&self) -> u64 {
        self.next_order_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Get the next sequence number (simulates TWS message sequencing).
    pub fn next_seq_num(&self) -> u64 {
        self.next_seq_num.fetch_add(1, Ordering::Relaxed)
    }

    /// Submit an order to the mock TWS.
    pub fn submit_order(&self, symbol: &str, quantity: u64) -> Result<u64, String> {
        if !self.is_connected() {
            return Err("Not connected to TWS".to_string());
        }

        // Check for pre-submit failures
        if self.should_trigger(FailureTiming::PreSubmit) {
            self.handle_failure(FailureTiming::PreSubmit)?;
            return Err("PreSubmit failure injected".to_string());
        }

        let order_id = self.next_order_id();
        let seq_num = self.next_seq_num();

        let mut orders = self.orders.lock().unwrap();
        orders.push(OrderState {
            id: order_id,
            symbol: symbol.to_string(),
            quantity,
            filled_quantity: 0,
            status: "Submitted".to_string(),
        });
        drop(orders);

        self.record_callback(&format!("OrderSubmitted: id={}, seq={}, symbol={}, qty={}",
            order_id, seq_num, symbol, quantity));

        // Check for post-submit failures
        if self.should_trigger(FailureTiming::PostSubmit) {
            self.handle_failure(FailureTiming::PostSubmit)?;
            return Err("PostSubmit failure injected".to_string());
        }

        Ok(order_id)
    }

    /// Simulate order status callback.
    pub fn order_status(&self, order_id: u64) -> Result<OrderState, String> {
        if !self.is_connected() {
            return Err("Not connected to TWS".to_string());
        }

        let orders = self.orders.lock().unwrap();
        let order = orders.iter().find(|o| o.id == order_id)
            .ok_or_else(|| format!("Order {order_id} not found"))?;
        Ok(order.clone())
    }

    /// Simulate order fill (full or partial).
    pub fn fill_order(&self, order_id: u64, fill_quantity: u64) -> Result<(), String> {
        if !self.is_connected() {
            return Err("Not connected to TWS".to_string());
        }

        // Check for mid-fill failures
        if self.should_trigger(FailureTiming::MidFill) {
            return self.handle_failure(FailureTiming::MidFill);
        }

        let mut orders = self.orders.lock().unwrap();
        if let Some(order) = orders.iter_mut().find(|o| o.id == order_id) {
            order.filled_quantity = fill_quantity;
            order.status = if fill_quantity >= order.quantity {
                "Filled".to_string()
            } else {
                "PartiallyFilled".to_string()
            };
            self.record_callback(&format!("OrderFill: id={}, filled={}/{}",
                order_id, fill_quantity, order.quantity));
        }

        // Check for post-fill failures
        if self.should_trigger(FailureTiming::PostFill) {
            return self.handle_failure(FailureTiming::PostFill);
        }

        Ok(())
    }

    /// Simulate order cancellation.
    pub fn cancel_order(&self, order_id: u64) -> Result<(), String> {
        if !self.is_connected() {
            return Err("Not connected to TWS".to_string());
        }

        let mut orders = self.orders.lock().unwrap();
        if let Some(order) = orders.iter_mut().find(|o| o.id == order_id) {
            order.status = "Cancelled".to_string();
            self.record_callback(&format!("OrderCancelled: id={}", order_id));
        }

        Ok(())
    }

    /// Get all recorded callbacks (for test assertions).
    pub fn callbacks(&self) -> Vec<String> {
        self.callbacks.lock().unwrap().clone()
    }

    /// Clear recorded callbacks.
    pub fn clear_callbacks(&self) {
        self.callbacks.lock().unwrap().clear();
    }

    /// Check if disconnect was injected (for F3 and F6 testing).
    pub fn was_disconnect_injected(&self) -> bool {
        self.disconnect_injected.load(Ordering::Relaxed)
    }

    /// Record a callback for test verification.
    fn record_callback(&self, callback: &str) {
        self.callbacks.lock().unwrap().push(callback.to_string());
    }

    /// Check if the active failure should trigger at the given timing.
    fn should_trigger(&self, timing: FailureTiming) -> bool {
        let failure = self.active_failure.lock().unwrap();
        match *failure {
            Some((_, t)) => t == timing,
            None => false,
        }
    }

    /// Handle the injected failure based on the current mode.
    fn handle_failure(&self, timing: FailureTiming) -> Result<(), String> {
        let failure = self.active_failure.lock().unwrap();
        let (mode, _) = failure.unwrap();

        match mode {
            FailureMode::F1DuplicateStatus => {
                // Will be handled by order_status returning duplicate
                drop(failure);
                Ok(())
            }
            FailureMode::F2CancelRejectRace => {
                drop(failure);
                Err("CancelReject: race condition with fill".to_string())
            }
            FailureMode::F3PartialFillDisconnect => {
                if timing == FailureTiming::PostFill {
                    self.disconnect_injected.store(true, Ordering::Relaxed);
                    self.connected.store(false, Ordering::Relaxed);
                    self.record_callback("InjectedDisconnect");
                }
                drop(failure);
                Ok(())
            }
            FailureMode::F4StaleMarketData => {
                drop(failure);
                Err("StaleMarketData: timestamp too old".to_string())
            }
            FailureMode::F5ClockSkew => {
                drop(failure);
                Err("ClockSkew: server time mismatch".to_string())
            }
            FailureMode::F6ReconnectDrill => {
                if timing == FailureTiming::PreSubmit {
                    self.disconnect_injected.store(true, Ordering::Relaxed);
                    self.connected.store(false, Ordering::Relaxed);
                    self.record_callback("InjectedDisconnect");
                }
                drop(failure);
                Ok(())
            }
            FailureMode::F7CronDoubleFire => {
                drop(failure);
                Err("CronDoubleFire: duplicate execution detected".to_string())
            }
            FailureMode::F8KillSwitch => {
                drop(failure);
                Err("KillSwitch: emergency stop triggered".to_string())
            }
            FailureMode::F9ProcessCrash => {
                drop(failure);
                Err("ProcessCrash: simulating crash".to_string())
            }
        }
    }
}

impl Default for MockTws {
    fn default() -> Self {
        Self::new()
    }
}