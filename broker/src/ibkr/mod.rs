//! Interactive Brokers (IBKR) broker implementation.

pub mod client;
pub mod market_data;
pub mod orders;

use nanobook::Symbol;
use std::thread;
use std::time::Duration;

use crate::Broker;
use crate::error::BrokerError;
use crate::types::*;
use client::IbkrClient;

/// Connection state tracking for IBKR broker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
}

#[derive(Debug, Clone)]
pub struct DiscrepancyReport {
    pub discrepancies: Vec<Discrepancy>,
    pub has_critical_issues: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Discrepancy {
    OrphanOrder {
        order_id: OrderId,
    },
    MissingOrder {
        order_id: OrderId,
    },
    OrderStatusMismatch {
        order_id: OrderId,
        local_status: String,
        broker_status: OrderState,
    },
    PositionMismatch {
        symbol: Symbol,
        local_quantity: i64,
        broker_quantity: i64,
    },
}

/// Interactive Brokers broker, wrapping the TWS/Gateway blocking API.
pub struct IbkrBroker {
    host: String,
    port: u16,
    client_id: i32,
    client: Option<IbkrClient>,
    connection_state: ConnectionState,
    reconciliation_blocked: bool,
}

impl IbkrBroker {
    /// Create a new IBKR broker handle (not yet connected).
    pub fn new(host: &str, port: u16, client_id: i32) -> Self {
        Self {
            host: host.to_string(),
            port,
            client_id,
            client: None,
            connection_state: ConnectionState::Disconnected,
            reconciliation_blocked: false,
        }
    }

    /// Get the underlying client (for advanced operations).
    /// Returns `None` if not connected.
    pub fn client(&self) -> Option<&IbkrClient> {
        self.client.as_ref()
    }

    fn require_client(&self) -> Result<&IbkrClient, BrokerError> {
        self.client.as_ref().ok_or(BrokerError::NotConnected)
    }

    /// Reconnect to IB Gateway/TWS after a disconnect.
    ///
    /// This method re-establishes the connection and queries current positions
    /// to detect any partial fills that may have occurred during the disconnect.
    ///
    /// # Returns
    /// * `Ok(Vec<Position>)` - Current positions after reconnect (for reconciliation)
    /// * `Err(BrokerError)` - If reconnection or position query fails
    pub fn reconnect(&mut self) -> Result<Vec<Position>, BrokerError> {
        let client = self.client.as_mut().ok_or(BrokerError::NotConnected)?;
        client.reconnect(&self.host, self.port, self.client_id)
    }

    /// Check if the broker is currently connected.
    pub fn is_connected(&self) -> bool {
        self.connection_state == ConnectionState::Connected
    }

    /// Get the current connection state.
    pub fn connection_state(&self) -> ConnectionState {
        self.connection_state
    }

    /// Check if reconciliation is currently blocked.
    pub fn is_reconciliation_blocked(&self) -> bool {
        self.reconciliation_blocked
    }

    /// Block reconciliation (e.g., after detecting critical discrepancies).
    pub fn block_reconciliation(&mut self) {
        self.reconciliation_blocked = true;
    }

    /// Unblock reconciliation (after manual review and resolution).
    pub fn unblock_reconciliation(&mut self) {
        self.reconciliation_blocked = false;
    }

    /// Reconnect with exponential backoff.
    ///
    /// This method attempts to reconnect with exponential backoff:
    /// 1s, 2s, 4s, 8s, 16s (max delay). Maximum 5 attempts.
    ///
    /// # Returns
    /// * `Ok(())` - Successfully reconnected
    /// * `Err(BrokerError::ReconnectFailed)` - All attempts failed
    pub fn reconnect_with_backoff(&mut self) -> Result<(), BrokerError> {
        const MAX_ATTEMPTS: u32 = 5;
        const INITIAL_DELAY_MS: u64 = 1000;
        const MAX_DELAY_MS: u64 = 16000;

        let mut last_error = String::from("unknown error");

        for attempt in 1..=MAX_ATTEMPTS {
            self.connection_state = ConnectionState::Reconnecting;

            // Calculate backoff delay (exponential, capped at MAX_DELAY_MS)
            let delay_ms = (INITIAL_DELAY_MS * 2_u64.pow(attempt - 1)).min(MAX_DELAY_MS);
            let delay = Duration::from_millis(delay_ms);

            // Sleep before attempting reconnect (except for first attempt)
            if attempt > 1 {
                thread::sleep(delay);
            }

            // Attempt reconnect
            match self.reconnect() {
                Ok(_) => {
                    self.connection_state = ConnectionState::Connected;
                    return Ok(());
                }
                Err(e) => {
                    last_error = e.to_string();
                    // Continue to next attempt
                }
            }
        }

        // All attempts failed
        self.connection_state = ConnectionState::Disconnected;
        Err(BrokerError::ReconnectFailed {
            attempts: MAX_ATTEMPTS,
            reason: last_error,
        })
    }

    pub fn reconcile_state(&mut self) -> Result<DiscrepancyReport, BrokerError> {
        let client = self.require_client()?;
        let open_orders = self.open_orders()?;
        let _positions = self.positions()?;
        let cached_orders = client.cached_orders()?;

        let mut discrepancies = Vec::new();
        for broker_order in &open_orders {
            match client.get_cached_order(broker_order.id.0 as i32)? {
                Some(cached) => {
                    let local_status = orders::map_ibkr_order_status(&cached.status);
                    if local_status != broker_order.status {
                        discrepancies.push(Discrepancy::OrderStatusMismatch {
                            order_id: broker_order.id,
                            local_status: cached.status,
                            broker_status: broker_order.status,
                        });
                    }
                }
                None => discrepancies.push(Discrepancy::OrphanOrder {
                    order_id: broker_order.id,
                }),
            }
        }

        for cached in cached_orders {
            if !open_orders
                .iter()
                .any(|broker_order| broker_order.id.0 == cached.order_id as u64)
            {
                discrepancies.push(Discrepancy::MissingOrder {
                    order_id: OrderId(cached.order_id as u64),
                });
            }
        }

        let has_critical_issues = !discrepancies.is_empty();

        // Block reconciliation if critical issues found
        if has_critical_issues {
            self.reconciliation_blocked = true;
        }

        Ok(DiscrepancyReport {
            has_critical_issues,
            discrepancies,
        })
    }
}

impl Broker for IbkrBroker {
    fn connect(&mut self) -> Result<(), BrokerError> {
        let client = IbkrClient::connect(&self.host, self.port, self.client_id)?;
        self.client = Some(client);
        self.connection_state = ConnectionState::Connected;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), BrokerError> {
        self.client = None;
        self.connection_state = ConnectionState::Disconnected;
        Ok(())
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        self.require_client()?.positions()
    }

    fn account(&self) -> Result<Account, BrokerError> {
        self.require_client()?.account_summary()
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        if self.reconciliation_blocked {
            return Err(BrokerError::Order(
                "Reconciliation blocked - manual review required".to_string(),
            ));
        }
        let client = self.require_client()?;
        client.submit_order(order)
    }

    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError> {
        let _client = self.require_client()?;
        // IBKR order status is tracked via the PlaceOrder subscription;
        // for now return a basic pending status. Full implementation requires
        // storing active order subscriptions.
        Ok(BrokerOrderStatus {
            id,
            status: OrderState::Submitted,
            filled_quantity: 0,
            remaining_quantity: 0,
            avg_fill_price_cents: 0,
        })
    }

    fn open_orders(&self) -> Result<Vec<BrokerOrderStatus>, BrokerError> {
        let client = self.require_client()?;
        match client.open_orders() {
            Ok(orders) => Ok(orders),
            Err(err) => {
                log::warn!("IBKR open-orders query failed: {err}");
                Ok(Vec::new())
            }
        }
    }

    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError> {
        let client = self.require_client()?;
        orders::cancel_order(client.inner(), id.0 as i32)
    }

    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        self.require_client()?.quote(symbol)
    }
}
