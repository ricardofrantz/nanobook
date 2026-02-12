//! Broker abstraction used by rebalancer execution.

use std::time::Duration;

use nanobook::Symbol;
use nanobook_broker::ibkr::client::IbkrClient;
use nanobook_broker::ibkr::orders;
use nanobook_broker::{
    BrokerSide,
    error::BrokerError,
    types::{Account, Position},
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
    fn execute_limit_order(
        &self,
        symbol: Symbol,
        side: BrokerSide,
        shares: u64,
        limit_price_cents: i64,
        timeout: Duration,
    ) -> BrokerResult<orders::OrderResult>;
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

    fn execute_limit_order(
        &self,
        symbol: Symbol,
        side: BrokerSide,
        shares: u64,
        limit_price_cents: i64,
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
            timeout,
        )
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
