//! Binance spot broker implementation.

pub mod auth;
pub mod client;
pub mod types;

use nanobook::Symbol;

use crate::Broker;
use crate::error::BrokerError;
use crate::types::*;
use client::BinanceClient;

/// Binance spot broker implementing the generic Broker trait.
///
/// Uses REST API for all operations. Blocking (sync) via reqwest::blocking.
pub struct BinanceBroker {
    api_key: String,
    secret_key: String,
    testnet: bool,
    client: Option<BinanceClient>,
    /// Symbol → Binance trading pair mapping.
    /// nanobook symbols are like "BTC", Binance needs "BTCUSDT".
    quote_asset: String,
}

impl BinanceBroker {
    /// Create a new Binance broker handle (not yet connected).
    ///
    /// `quote_asset` is the quote currency (default "USDT") appended to
    /// nanobook symbols to form Binance trading pairs (e.g., "BTC" → "BTCUSDT").
    pub fn new(api_key: &str, secret_key: &str, testnet: bool) -> Self {
        Self {
            api_key: api_key.to_string(),
            secret_key: secret_key.to_string(),
            testnet,
            client: None,
            quote_asset: "USDT".to_string(),
        }
    }

    /// Set the quote asset (default "USDT").
    pub fn with_quote_asset(mut self, quote: &str) -> Self {
        self.quote_asset = quote.to_string();
        self
    }

    /// Convert a nanobook Symbol to a Binance trading pair string.
    fn to_binance_symbol(&self, symbol: &Symbol) -> String {
        format!("{}{}", symbol.as_str(), self.quote_asset)
    }

    fn require_client(&self) -> Result<&BinanceClient, BrokerError> {
        self.client.as_ref().ok_or(BrokerError::NotConnected)
    }

    /// Parse a decimal string to cents (e.g., "185.50" → 18550).
    fn parse_price_cents(s: &str) -> i64 {
        let val: f64 = s.parse().unwrap_or(0.0);
        (val * 100.0) as i64
    }
}

impl Broker for BinanceBroker {
    fn connect(&mut self) -> Result<(), BrokerError> {
        let client = BinanceClient::new(&self.api_key, &self.secret_key, self.testnet);
        client.ping()?;
        self.client = Some(client);
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), BrokerError> {
        self.client = None;
        Ok(())
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        let client = self.require_client()?;
        let info = client.account_info()?;

        let positions = info
            .balances
            .iter()
            .filter_map(|b| {
                let free: f64 = b.free.parse().unwrap_or(0.0);
                let locked: f64 = b.locked.parse().unwrap_or(0.0);
                let total = free + locked;
                if total <= 0.0 {
                    return None;
                }
                let sym = Symbol::try_new(&b.asset)?;
                // Crypto positions are always positive (long), quantity in smallest unit
                let qty = (total * 1e8) as i64; // satoshis for BTC, etc.
                Some(Position {
                    symbol: sym,
                    quantity: qty,
                    avg_cost_cents: 0,     // Binance doesn't track avg cost
                    market_value_cents: 0, // would need live prices
                    unrealized_pnl_cents: 0,
                })
            })
            .collect();

        Ok(positions)
    }

    fn account(&self) -> Result<Account, BrokerError> {
        let client = self.require_client()?;
        let info = client.account_info()?;

        // Sum USDT-equivalent balance as a rough equity estimate
        let usdt_balance: f64 = info
            .balances
            .iter()
            .filter(|b| b.asset == self.quote_asset)
            .map(|b| {
                let free: f64 = b.free.parse().unwrap_or(0.0);
                let locked: f64 = b.locked.parse().unwrap_or(0.0);
                free + locked
            })
            .sum();

        let equity_cents = (usdt_balance * 100.0) as i64;

        Ok(Account {
            equity_cents,
            buying_power_cents: equity_cents,
            cash_cents: equity_cents,
            gross_position_value_cents: 0,
        })
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        let client = self.require_client()?;
        let binance_sym = self.to_binance_symbol(&order.symbol);
        let side = match order.side {
            BrokerSide::Buy => "BUY",
            BrokerSide::Sell => "SELL",
        };

        let (order_type, price, tif) = match order.order_type {
            BrokerOrderType::Market => ("MARKET", None, None),
            BrokerOrderType::Limit(p) => {
                let price_str = format!("{:.2}", p.0 as f64 / 100.0);
                ("LIMIT", Some(price_str), Some("GTC"))
            }
        };

        let qty_str = format!("{}", order.quantity);

        let resp = client.submit_order(
            &binance_sym,
            side,
            order_type,
            &qty_str,
            price.as_deref(),
            tif,
        )?;

        Ok(OrderId(resp.order_id))
    }

    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError> {
        // Binance requires the symbol to query order status.
        // Since we only have the order ID, return a basic status.
        // Full implementation would need a local order cache.
        Ok(BrokerOrderStatus {
            id,
            status: OrderState::Submitted,
            filled_quantity: 0,
            remaining_quantity: 0,
            avg_fill_price_cents: 0,
        })
    }

    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError> {
        // Binance requires symbol + orderId. Without a local cache,
        // this is a placeholder. Full implementation would store
        // symbol mappings from submit_order.
        let _ = id;
        Err(BrokerError::Order(
            "cancel requires symbol — use BinanceBroker.cancel_order_with_symbol() instead".into(),
        ))
    }

    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        let client = self.require_client()?;
        let binance_sym = self.to_binance_symbol(symbol);
        let ticker = client.book_ticker(&binance_sym)?;

        let bid = Self::parse_price_cents(&ticker.bid_price);
        let ask = Self::parse_price_cents(&ticker.ask_price);
        let last = (bid + ask) / 2; // Binance bookTicker doesn't have last; use mid

        Ok(Quote {
            symbol: *symbol,
            bid_cents: bid,
            ask_cents: ask,
            last_cents: last,
            volume: 0,
        })
    }
}
