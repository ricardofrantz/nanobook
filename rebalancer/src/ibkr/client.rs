//! IBKR connection, position fetching, market data, and account summary.

use ibapi::accounts::types::AccountGroup;
use ibapi::accounts::{AccountSummaryResult, PositionUpdate};
use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::{TickType, TickTypes};
use log::{debug, info, warn};
use nanobook::Symbol;

use crate::config::ConnectionConfig;
use crate::diff::CurrentPosition;
use crate::error::{Error, Result};

/// Account summary data from IBKR.
#[derive(Debug, Clone)]
pub struct AccountSummary {
    pub equity_cents: i64,
    pub cash_cents: i64,
    pub buying_power_cents: i64,
}

/// Wraps the ibapi blocking client with convenience methods.
pub struct IbkrClient {
    client: Client,
}

impl IbkrClient {
    /// Connect to IB Gateway/TWS.
    pub fn connect(config: &ConnectionConfig) -> Result<Self> {
        let address = format!("{}:{}", config.host, config.port);
        info!("Connecting to IB Gateway at {address}...");

        let client = Client::connect(&address, config.client_id)
            .map_err(|e| Error::Connection(format!("failed to connect to {address}: {e}")))?;

        info!("Connected (client_id={})", config.client_id);
        Ok(Self { client })
    }

    /// Get the underlying ibapi client (for order submission).
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Fetch current positions from IBKR.
    pub fn positions(&self) -> Result<Vec<CurrentPosition>> {
        let subscription = self
            .client
            .positions()
            .map_err(|e| Error::Connection(format!("failed to request positions: {e}")))?;

        let mut positions = Vec::new();
        for update in subscription {
            match update {
                PositionUpdate::Position(pos) => {
                    let symbol_str = pos.contract.symbol.to_string();
                    if let Some(sym) = Symbol::try_new(&symbol_str) {
                        debug!(
                            "Position: {} qty={} avg_cost={:.2}",
                            sym, pos.position, pos.average_cost
                        );
                        positions.push(CurrentPosition {
                            symbol: sym,
                            quantity: pos.position as i64,
                            avg_cost_cents: (pos.average_cost * 100.0) as i64,
                        });
                    } else {
                        warn!("Skipping symbol '{symbol_str}' (> 8 bytes)");
                    }
                }
                PositionUpdate::PositionEnd => break,
            }
        }

        info!("Fetched {} positions", positions.len());
        Ok(positions)
    }

    /// Fetch account summary (equity, cash, buying power).
    pub fn account_summary(&self) -> Result<AccountSummary> {
        let group = AccountGroup("All".to_string());
        let tags = &[
            "NetLiquidation",
            "TotalCashValue",
            "BuyingPower",
        ];

        let subscription = self
            .client
            .account_summary(&group, tags)
            .map_err(|e| Error::Connection(format!("failed to request account summary: {e}")))?;

        let mut equity = 0.0_f64;
        let mut cash = 0.0_f64;
        let mut buying_power = 0.0_f64;

        for result in subscription {
            match result {
                AccountSummaryResult::Summary(s) => {
                    debug!("Account: {}={} {}", s.tag, s.value, s.currency);
                    let value: f64 = s.value.parse().unwrap_or(0.0);
                    match s.tag.as_str() {
                        "NetLiquidation" => equity = value,
                        "TotalCashValue" => cash = value,
                        "BuyingPower" => buying_power = value,
                        _ => {}
                    }
                }
                AccountSummaryResult::End => break,
            }
        }

        let summary = AccountSummary {
            equity_cents: (equity * 100.0) as i64,
            cash_cents: (cash * 100.0) as i64,
            buying_power_cents: (buying_power * 100.0) as i64,
        };

        info!(
            "Account: equity=${:.2}, cash=${:.2}, buying_power=${:.2}",
            equity, cash, buying_power
        );

        Ok(summary)
    }

    /// Fetch live prices (bid/ask midpoint) for a set of symbols.
    ///
    /// Returns (symbol, price_cents) pairs. Uses market data snapshots.
    pub fn prices(&self, symbols: &[Symbol]) -> Result<Vec<(Symbol, i64)>> {
        let mut prices = Vec::with_capacity(symbols.len());

        for &sym in symbols {
            let contract = Contract::stock(sym.as_str()).build();
            match self.fetch_snapshot_price(&contract) {
                Ok(mid_cents) => {
                    debug!("{}: ${:.2}", sym, mid_cents as f64 / 100.0);
                    prices.push((sym, mid_cents));
                }
                Err(e) => {
                    warn!("Failed to get price for {sym}: {e}");
                    return Err(Error::Connection(format!(
                        "failed to get price for {sym}: {e}"
                    )));
                }
            }
        }

        Ok(prices)
    }

    /// Fetch a single snapshot price (bid/ask midpoint) for a contract.
    fn fetch_snapshot_price(&self, contract: &Contract) -> Result<i64> {
        let subscription = self
            .client
            .market_data(contract)
            .snapshot()
            .subscribe()
            .map_err(|e| Error::Connection(format!("market data request failed: {e}")))?;

        let mut bid = None;
        let mut ask = None;
        let mut last = None;

        for tick in subscription {
            match tick {
                TickTypes::Price(price_tick) => {
                    match price_tick.tick_type {
                        TickType::Bid => bid = Some(price_tick.price),
                        TickType::Ask => ask = Some(price_tick.price),
                        TickType::Last => last = Some(price_tick.price),
                        _ => {}
                    }
                }
                TickTypes::PriceSize(ps) => {
                    match ps.price_tick_type {
                        TickType::Bid => bid = Some(ps.price),
                        TickType::Ask => ask = Some(ps.price),
                        TickType::Last => last = Some(ps.price),
                        _ => {}
                    }
                }
                TickTypes::SnapshotEnd => break,
                _ => {}
            }
        }

        // Prefer bid/ask midpoint, fall back to last price
        let price = match (bid, ask) {
            (Some(b), Some(a)) if b > 0.0 && a > 0.0 => (b + a) / 2.0,
            _ => last.unwrap_or(0.0),
        };

        if price <= 0.0 {
            return Err(Error::Connection("no valid price received".into()));
        }

        Ok((price * 100.0) as i64)
    }
}
