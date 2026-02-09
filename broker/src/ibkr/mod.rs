//! Interactive Brokers (IBKR) broker implementation.

pub mod client;
pub mod orders;

use nanobook::Symbol;

use crate::Broker;
use crate::error::BrokerError;
use crate::types::*;
use client::IbkrClient;

/// Interactive Brokers broker, wrapping the TWS/Gateway blocking API.
pub struct IbkrBroker {
    host: String,
    port: u16,
    client_id: i32,
    client: Option<IbkrClient>,
}

impl IbkrBroker {
    /// Create a new IBKR broker handle (not yet connected).
    pub fn new(host: &str, port: u16, client_id: i32) -> Self {
        Self {
            host: host.to_string(),
            port,
            client_id,
            client: None,
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
}

impl Broker for IbkrBroker {
    fn connect(&mut self) -> Result<(), BrokerError> {
        let client = IbkrClient::connect(&self.host, self.port, self.client_id)?;
        self.client = Some(client);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), BrokerError> {
        self.client = None;
        Ok(())
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        self.require_client()?.positions()
    }

    fn account(&self) -> Result<Account, BrokerError> {
        self.require_client()?.account_summary()
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        let client = self.require_client()?;
        orders::submit_order(client.inner(), order)
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

    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError> {
        let client = self.require_client()?;
        orders::cancel_order(client.inner(), id.0 as i32);
        Ok(())
    }

    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        self.require_client()?.quote(symbol)
    }
}
