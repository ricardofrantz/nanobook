//! Broker abstraction used by rebalancer execution.

use std::time::Duration;

use nanobook::Symbol;
use nanobook_broker::ibkr::client::IbkrClient;
use nanobook_broker::ibkr::orders;
use nanobook_broker::{
    BrokerSide, ClientOrderId,
    error::BrokerError,
    types::{Account, Position, Quote},
};

use crate::config::Config;
use crate::error::{Error, Result};

pub type BrokerResult<T> = std::result::Result<T, BrokerError>;

pub fn as_connection_error<T>(result: BrokerResult<T>) -> Result<T> {
    result.map_err(|e| Error::Connection(e.to_string()))
}

/// Minimal broker API needed by the rebalancer runtime.
pub trait BrokerGateway {
    fn account_summary(&self) -> BrokerResult<Account>;
    fn positions(&self) -> BrokerResult<Vec<Position>>;
    fn prices(&self, symbols: &[Symbol]) -> BrokerResult<Vec<(Symbol, i64)>>;
    fn quotes(&self, symbols: &[Symbol]) -> BrokerResult<Vec<Quote>>;
    fn execute_limit_order(
        &self,
        symbol: Symbol,
        side: BrokerSide,
        shares: u64,
        limit_price_cents: i64,
        client_order_id: Option<&ClientOrderId>,
        timeout: Duration,
    ) -> BrokerResult<orders::OrderResult>;

    /// Cancel a pending broker order.
    ///
    /// The rebalancer does not call this in the normal order path today
    /// (timeouts are handled inside `execute_limit_order`), but exposing it here
    /// lets cancellation paths use the same write-ahead audit discipline.
    fn cancel_order(&self, order_id: u64) -> BrokerResult<()> {
        let _ = order_id;
        Err(BrokerError::Other(
            "cancel_order is not supported by this broker gateway".into(),
        ))
    }
}

impl BrokerGateway for IbkrClient {
    fn account_summary(&self) -> BrokerResult<Account> {
        self.account_summary()
    }

    fn positions(&self) -> BrokerResult<Vec<Position>> {
        self.positions()
    }

    fn prices(&self, symbols: &[Symbol]) -> BrokerResult<Vec<(Symbol, i64)>> {
        self.prices(symbols)
    }

    fn quotes(&self, symbols: &[Symbol]) -> BrokerResult<Vec<Quote>> {
        let mut quotes = Vec::with_capacity(symbols.len());
        for &sym in symbols {
            quotes.push(self.quote(&sym)?);
        }
        Ok(quotes)
    }

    fn execute_limit_order(
        &self,
        symbol: Symbol,
        side: BrokerSide,
        shares: u64,
        limit_price_cents: i64,
        client_order_id: Option<&ClientOrderId>,
        timeout: Duration,
    ) -> BrokerResult<orders::OrderResult> {
        let shares = i64::try_from(shares)
            .map_err(|_| BrokerError::Order("share quantity exceeds i64::MAX".into()))?;

        orders::execute_limit_order(
            self.inner(),
            symbol,
            side,
            shares,
            limit_price_cents,
            client_order_id,
            timeout,
            None, // TODO: pass dedup cache when available
        )
    }

    fn cancel_order(&self, order_id: u64) -> BrokerResult<()> {
        let order_id = i32::try_from(order_id)
            .map_err(|_| BrokerError::Order("order id exceeds i32::MAX".into()))?;
        orders::cancel_order(self.inner(), order_id)
    }
}

pub fn connect_ibkr(config: &Config) -> Result<Box<dyn BrokerGateway>> {
    IbkrClient::connect(
        &config.connection.host,
        config.connection.port,
        config.connection.client_id,
    )
    .map(|client| Box::new(client) as Box<dyn BrokerGateway>)
    .map_err(|e| Error::Connection(e.to_string()))
}
