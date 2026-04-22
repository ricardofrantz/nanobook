//! IBKR connection, position fetching, market data, and account summary.

use std::collections::HashMap;
use std::sync::Mutex;

use ibapi::accounts::types::AccountGroup;
use ibapi::accounts::{AccountSummaryResult, PositionUpdate};
use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::market_data::realtime::{TickType, TickTypes};
use log::{debug, info, warn};
use nanobook::Symbol;

use crate::error::BrokerError;
use crate::parse::parse_f64_or_warn;
use crate::types::{Account, BestQuote, BrokerOrder, OrderId, Position, Quote, f64_cents_checked};

use super::market_data::best_quote_from_quote;
use super::orders;

/// Wraps the ibapi blocking client with convenience methods.
///
/// Unlike `BinanceBroker`, `IbkrClient` holds no credentials —
/// IBKR authentication happens at the TWS/Gateway socket layer
/// via `(host, port, client_id)` rather than via API-key strings
/// in process memory. There is nothing this struct owns that
/// benefits from `ZeroizeOnDrop`, so the derive is deliberately
/// absent. If a future refactor adds a secret-bearing field,
/// re-evaluate: ibapi's `Client` does not implement `Zeroize` and
/// would need `#[zeroize(skip)]`.
pub struct IbkrClient {
    client: Client,
    best_quotes: Mutex<HashMap<Symbol, BestQuote>>,
}

impl IbkrClient {
    /// Connect to IB Gateway/TWS.
    pub fn connect(host: &str, port: u16, client_id: i32) -> Result<Self, BrokerError> {
        let address = format!("{host}:{port}");
        info!("Connecting to IB Gateway at {address}...");

        let client = Client::connect(&address, client_id)
            .map_err(|e| BrokerError::Connection(format!("failed to connect to {address}: {e}")))?;

        info!("Connected (client_id={client_id})");
        Ok(Self {
            client,
            best_quotes: Mutex::new(HashMap::new()),
        })
    }

    /// Get the underlying ibapi client (for order submission).
    pub fn inner(&self) -> &Client {
        &self.client
    }

    /// Submit an order using the current best-quote cache when available.
    pub fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        let best_quote = self.cached_best_quote(&order.symbol)?;
        orders::submit_order(&self.client, order, best_quote.as_ref())
    }

    fn cached_best_quote(&self, symbol: &Symbol) -> Result<Option<BestQuote>, BrokerError> {
        let quotes = self
            .best_quotes
            .lock()
            .map_err(|_| BrokerError::Other("best quote cache poisoned".into()))?;
        Ok(quotes.get(symbol).copied())
    }

    fn cache_best_quote(&self, symbol: Symbol, quote: BestQuote) -> Result<(), BrokerError> {
        let mut quotes = self
            .best_quotes
            .lock()
            .map_err(|_| BrokerError::Other("best quote cache poisoned".into()))?;
        quotes.insert(symbol, quote);
        Ok(())
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
                        let avg_cost_cents =
                            f64_cents_checked(pos.average_cost, "ibkr position.average_cost")?;
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

        // Demoted to debug in S7: the position count is a coarse
        // signal of account activity and shouldn't appear in
        // aggregated info-level logs.
        debug!("Fetched {} positions", positions.len());
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
                    // Each known tag routes to its own destination AND its
                    // own `field` label for the parse-failure warning, so
                    // a malformed NetLiquidation vs. BuyingPower surfaces
                    // distinguishably in logs.
                    match s.tag.as_str() {
                        "NetLiquidation" => {
                            equity = parse_f64_or_warn(&s.value, "ibkr account.NetLiquidation");
                        }
                        "TotalCashValue" => {
                            cash = parse_f64_or_warn(&s.value, "ibkr account.TotalCashValue");
                        }
                        "BuyingPower" => {
                            buying_power = parse_f64_or_warn(&s.value, "ibkr account.BuyingPower");
                        }
                        _ => {}
                    }
                }
                AccountSummaryResult::End => break,
            }
        }

        let account = Account {
            equity_cents: f64_cents_checked(equity, "ibkr NetLiquidation")?,
            cash_cents: f64_cents_checked(cash, "ibkr TotalCashValue")?,
            buying_power_cents: f64_cents_checked(buying_power, "ibkr BuyingPower")?,
            gross_position_value_cents: f64_cents_checked(
                equity - cash,
                "ibkr gross_position_value",
            )?,
        };

        // Demoted to debug in S7: equity/cash/buying-power are
        // financial PII and must not appear in aggregated info-level
        // logs. Set `RUST_LOG=debug` locally if you need to see them
        // while debugging.
        debug!(
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

        let bid_cents = bid
            .map(|b| f64_cents_checked(b, "ibkr tick.bid"))
            .transpose()?
            .unwrap_or(0);
        let ask_cents = ask
            .map(|a| f64_cents_checked(a, "ibkr tick.ask"))
            .transpose()?
            .unwrap_or(0);
        let last_cents = last
            .map(|l| f64_cents_checked(l, "ibkr tick.last"))
            .transpose()?
            .unwrap_or(0);

        // Require at least one valid price
        if bid_cents <= 0 && ask_cents <= 0 && last_cents <= 0 {
            return Err(BrokerError::Connection("no valid price received".into()));
        }

        let quote = Quote {
            symbol: *symbol,
            bid_cents,
            ask_cents,
            last_cents,
            volume: 0, // snapshot doesn't provide volume
        };

        if let Some(best_quote) = best_quote_from_quote(&quote) {
            self.cache_best_quote(*symbol, best_quote)?;
        }

        Ok(quote)
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
