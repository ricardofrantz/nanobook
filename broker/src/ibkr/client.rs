//! IBKR connection, position fetching, market data, and account summary.

use ibapi::accounts::types::AccountGroup;
use ibapi::accounts::{AccountSummaryResult, PositionUpdate};
use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::{TickType, TickTypes};
use log::{debug, info, warn};
use nanobook::Symbol;

use crate::error::BrokerError;
use crate::types::{Account, Position, Quote};

/// Wraps the ibapi blocking client with convenience methods.
pub struct IbkrClient {
    client: Client,
}

impl IbkrClient {
    /// Connect to IB Gateway/TWS.
    pub fn connect(host: &str, port: u16, client_id: i32) -> Result<Self, BrokerError> {
        let address = format!("{host}:{port}");
        info!("Connecting to IB Gateway at {address}...");

        let client = Client::connect(&address, client_id)
            .map_err(|e| BrokerError::Connection(format!("failed to connect to {address}: {e}")))?;

        info!("Connected (client_id={client_id})");
        Ok(Self { client })
    }

    /// Get the underlying ibapi client (for order submission).
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Fetch current positions from IBKR.
    pub fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        let subscription = self
            .client
            .positions()
            .map_err(|e| BrokerError::Connection(format!("failed to request positions: {e}")))?;

        let mut positions = Vec::new();
        for update in subscription {
            match update {
                PositionUpdate::Position(pos) => {
                    let symbol_str = pos.contract.symbol.to_string();
                    if let Some(sym) = Symbol::try_new(&symbol_str) {
                        let qty = pos.position as i64;
                        let avg_cost_cents = (pos.average_cost * 100.0) as i64;
                        debug!(
                            "Position: {} qty={} avg_cost={:.2}",
                            sym, qty, pos.average_cost
                        );
                        positions.push(Position {
                            symbol: sym,
                            quantity: qty,
                            avg_cost_cents,
                            market_value_cents: qty.abs() * avg_cost_cents, // approximate
                            unrealized_pnl_cents: 0, // would need live prices for exact value
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
    pub fn account_summary(&self) -> Result<Account, BrokerError> {
        let group = AccountGroup("All".to_string());
        let tags = &["NetLiquidation", "TotalCashValue", "BuyingPower"];

        let subscription = self.client.account_summary(&group, tags).map_err(|e| {
            BrokerError::Connection(format!("failed to request account summary: {e}"))
        })?;

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

        let account = Account {
            equity_cents: (equity * 100.0) as i64,
            cash_cents: (cash * 100.0) as i64,
            buying_power_cents: (buying_power * 100.0) as i64,
            gross_position_value_cents: ((equity - cash) * 100.0) as i64,
        };

        info!(
            "Account: equity=${:.2}, cash=${:.2}, buying_power=${:.2}",
            equity, cash, buying_power
        );

        Ok(account)
    }

    /// Fetch a live quote for a symbol.
    pub fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        let contract = Contract::stock(symbol.as_str()).build();
        let subscription = self
            .client
            .market_data(&contract)
            .snapshot()
            .subscribe()
            .map_err(|e| BrokerError::Connection(format!("market data request failed: {e}")))?;

        let mut bid = None;
        let mut ask = None;
        let mut last = None;

        for tick in subscription {
            match tick {
                TickTypes::Price(price_tick) => match price_tick.tick_type {
                    TickType::Bid => bid = Some(price_tick.price),
                    TickType::Ask => ask = Some(price_tick.price),
                    TickType::Last => last = Some(price_tick.price),
                    _ => {}
                },
                TickTypes::PriceSize(ps) => match ps.price_tick_type {
                    TickType::Bid => bid = Some(ps.price),
                    TickType::Ask => ask = Some(ps.price),
                    TickType::Last => last = Some(ps.price),
                    _ => {}
                },
                TickTypes::SnapshotEnd => break,
                _ => {}
            }
        }

        let bid_cents = bid.map(|b| (b * 100.0) as i64).unwrap_or(0);
        let ask_cents = ask.map(|a| (a * 100.0) as i64).unwrap_or(0);
        let last_cents = last.map(|l| (l * 100.0) as i64).unwrap_or(0);

        // Require at least one valid price
        if bid_cents <= 0 && ask_cents <= 0 && last_cents <= 0 {
            return Err(BrokerError::Connection("no valid price received".into()));
        }

        Ok(Quote {
            symbol: *symbol,
            bid_cents,
            ask_cents,
            last_cents,
            volume: 0, // snapshot doesn't provide volume
        })
    }

    /// Fetch bid/ask midpoint price for a symbol, in cents.
    pub fn mid_price(&self, symbol: &Symbol) -> Result<i64, BrokerError> {
        let q = self.quote(symbol)?;
        let mid = match (q.bid_cents, q.ask_cents) {
            (b, a) if b > 0 && a > 0 => b + (a - b) / 2,
            (b, _) if b > 0 => b,
            (_, a) if a > 0 => a,
            _ => q.last_cents,
        };
        if mid <= 0 {
            return Err(BrokerError::Connection("no valid price received".into()));
        }
        Ok(mid)
    }

    /// Fetch live prices (bid/ask midpoint) for a set of symbols.
    pub fn prices(&self, symbols: &[Symbol]) -> Result<Vec<(Symbol, i64)>, BrokerError> {
        let mut prices = Vec::with_capacity(symbols.len());
        for &sym in symbols {
            let mid = self.mid_price(&sym)?;
            debug!("{}: ${:.2}", sym, mid as f64 / 100.0);
            prices.push((sym, mid));
        }
        Ok(prices)
    }
}
