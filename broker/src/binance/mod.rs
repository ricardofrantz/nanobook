//! Binance spot broker implementation.

pub mod audit;
pub mod auth;
pub mod cache;
pub mod client;
pub mod types;
pub mod websocket;

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use chrono::{DateTime, Utc};
use nanobook::Symbol;
use uuid::Uuid;

use crate::Broker;
use crate::error::BrokerError;
use crate::parse::parse_f64_or_warn;
use crate::types::*;
pub use audit::{check_audit_log_for_sequence, log_idempotency_rejection, log_order_submitted};
pub use cache::BinanceOrderCache;
pub use types::{Discrepancy, DiscrepancyReport};
use client::BinanceClient;

#[derive(Debug, Clone)]
pub struct CachedOrder {
    pub symbol: Symbol,
    pub quantity: i64,
    pub side: BrokerSide,
    pub status: OrderState,
    pub binance_order_id: String,
    pub client_order_id: Option<String>,
    pub submitted_at: DateTime<Utc>,
}

/// Binance spot broker implementing the generic Broker trait.
///
/// Uses REST API for all operations. Blocking (sync) via reqwest::blocking.
///
/// `api_key` and `secret_key` are scrubbed in memory on drop via
/// [`ZeroizeOnDrop`](zeroize::ZeroizeOnDrop). `testnet` and
/// `quote_asset` carry no secrets and are marked
/// `#[zeroize(skip)]`; `client: Option<BinanceClient>` is also
/// skipped because `BinanceClient` already scrubs its own copy of
/// the credentials on drop.
///
/// ## PyO3 caveat
///
/// Scrubbing the Rust-side copies of the credentials does NOT
/// scrub the originals if they came in through PyO3 as `&str`
/// parameters — those live in a `PyString` owned by the Python
/// interpreter and are out of Rust's reach. Pass credentials via
/// environment variables (read on the Rust side from
/// `std::env::var`) to keep them from ever transiting a
/// `PyString`. See `broker/README.md` for details.
#[derive(zeroize::ZeroizeOnDrop)]
pub struct BinanceBroker {
    api_key: String,
    secret_key: String,
    #[zeroize(skip)]
    testnet: bool,
    #[zeroize(skip)]
    client: Option<BinanceClient>,
    /// Symbol → Binance trading pair mapping.
    /// nanobook symbols are like "BTC", Binance needs "BTCUSDT".
    #[zeroize(skip)]
    quote_asset: String,
    #[zeroize(skip)]
    order_cache: Mutex<BinanceOrderCache>,
    /// Optional path to audit log file for idempotency tracking.
    #[zeroize(skip)]
    audit_log_path: Option<PathBuf>,
    /// Optional sequence number for client order ID generation and audit logging.
    #[zeroize(skip)]
    sequence_number: Option<u64>,
    /// Flag indicating if reconciliation is blocked due to detected discrepancies.
    #[zeroize(skip)]
    reconciliation_blocked: bool,
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
            order_cache: Mutex::new(BinanceOrderCache::new()),
            audit_log_path: None,
            sequence_number: None,
            reconciliation_blocked: false,
        }
    }

    /// Set the quote asset (default "USDT").
    pub fn with_quote_asset(mut self, quote: &str) -> Self {
        self.quote_asset = quote.to_string();
        self
    }

    /// Set the audit log path for idempotency tracking.
    pub fn with_audit_log_path(mut self, path: PathBuf) -> Self {
        self.audit_log_path = Some(path);
        self
    }

    /// Set the sequence number for client order ID generation and audit logging.
    pub fn with_sequence_number(mut self, seq: u64) -> Self {
        self.sequence_number = Some(seq);
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
    ///
    /// On parse failure, [`parse_f64_or_warn`] emits a
    /// `log::warn!` naming the field and returns `0.0`, so the
    /// whole function returns `Ok(0)` — a plausible zero that lets
    /// error recovery continue. If the parsed `f64` is non-finite
    /// or overflows `i64` after the ×100 scaling,
    /// [`f64_cents_checked`] surfaces it as an explicit
    /// `BrokerError`.
    fn parse_price_cents(s: &str, field: &'static str) -> Result<i64, BrokerError> {
        let val = parse_f64_or_warn(s, field);
        f64_cents_checked(val, field)
    }

    pub fn cache_order(
        &self,
        order_id: OrderId,
        symbol: Symbol,
        quantity: i64,
        side: BrokerSide,
        client_order_id: Option<String>,
    ) {
        self.cache_order_with_binance_id(
            order_id,
            symbol,
            quantity,
            side,
            order_id.0.to_string(),
            client_order_id,
            Utc::now(),
        );
    }

    fn cache_order_with_binance_id(
        &self,
        order_id: OrderId,
        symbol: Symbol,
        quantity: i64,
        side: BrokerSide,
        binance_order_id: String,
        client_order_id: Option<String>,
        submitted_at: DateTime<Utc>,
    ) {
        let mut cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        cache.orders.insert(
            order_id,
            CachedOrder {
                symbol,
                quantity,
                side,
                status: OrderState::Submitted,
                binance_order_id,
                client_order_id,
                submitted_at,
            },
        );
    }

    pub fn update_cached_order_status(&self, order_id: OrderId, status: OrderState) {
        let mut cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        if let Some(order) = cache.orders.get_mut(&order_id) {
            order.status = status;
        }
    }

    pub fn get_cached_order(&self, order_id: OrderId) -> Option<CachedOrder> {
        let cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        cache.orders.get(&order_id).cloned()
    }

    pub fn clear_cache(&self) {
        let mut cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        cache.orders.clear();
    }

    pub fn load_cache_from_disk(&self, path: &Path) -> Result<(), BrokerError> {
        let loaded = BinanceOrderCache::load_from_disk(path)?;
        let mut cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        *cache = loaded;
        Ok(())
    }

    pub fn save_cache_to_disk(&self, path: &Path) -> Result<(), BrokerError> {
        let cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        cache.save_to_disk(path)
    }

    /// Generate a unique client order ID for idempotency.
    ///
    /// Format: "nanobook-{short_uuid}-{sequence_number}"
    /// The UUID ensures uniqueness across runs, and the sequence number
    /// provides traceability for ordering operations.
    /// Uses first 16 hex characters of UUID (without hyphens) to fit within 36-character limit.
    pub fn generate_client_order_id(&self, sequence_number: u64) -> String {
        let uuid = Uuid::new_v4();
        let short_uuid = uuid
            .to_string()
            .chars()
            .filter(|c| *c != '-')
            .take(16)
            .collect::<String>();
        format!("nanobook-{}-{}", short_uuid, sequence_number)
    }

    /// Check if a client order ID already exists in the order cache.
    ///
    /// Returns true if a duplicate is found, false otherwise.
    pub fn check_duplicate_client_order_id(&self, client_order_id: &str) -> bool {
        let cache = self
            .order_cache
            .lock()
            .expect("Binance order cache mutex poisoned");
        cache
            .orders
            .values()
            .any(|order| order.client_order_id.as_deref() == Some(client_order_id))
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

    /// Reconcile local state with Binance account state.
    ///
    /// Queries account info from Binance and compares against local order cache
    /// to detect discrepancies such as orphan orders, missing orders, and position mismatches.
    ///
    /// # Returns
    /// * `Ok(DiscrepancyReport)` - Report of any discrepancies found
    /// * `Err(BrokerError)` - If query fails or not connected
    pub fn reconcile_state(&mut self) -> Result<DiscrepancyReport, BrokerError> {
        let client = self.require_client()?;
        let info = client.account_info()?;

        let mut discrepancies = Vec::new();

        // Get cached orders as a cloned vector
        let cached_orders: Vec<(OrderId, CachedOrder)> = {
            let cache = self
                .order_cache
                .lock()
                .expect("Binance order cache mutex poisoned");
            cache.orders.iter().map(|(k, v)| (*k, v.clone())).collect()
        };

        // Check for orphan orders (orders on broker but not in cache)
        for broker_order in &info.open_orders {
            let order_id = OrderId(broker_order.order_id);
            let found = cached_orders
                .iter()
                .any(|(id, _)| id.0 == broker_order.order_id);
            if !found {
                discrepancies.push(Discrepancy::OrphanOrder { order_id });
            }
        }

        // Check for missing orders (orders in cache but not on broker)
        for (order_id, cached_order) in &cached_orders {
            let found = info
                .open_orders
                .iter()
                .any(|broker_order| broker_order.order_id == order_id.0);
            if !found && cached_order.status == OrderState::Submitted {
                discrepancies.push(Discrepancy::MissingOrder { order_id: *order_id });
            }
        }

        // Check for order status mismatches
        for broker_order in &info.open_orders {
            let order_id = OrderId(broker_order.order_id);
            if let Some((_, cached_order)) = cached_orders
                .iter()
                .find(|(id, _)| id.0 == broker_order.order_id)
            {
                let broker_status = match broker_order.status.as_str() {
                    "NEW" => OrderState::Submitted,
                    "PARTIALLY_FILLED" => OrderState::PartiallyFilled,
                    "FILLED" => OrderState::Filled,
                    "CANCELED" => OrderState::Cancelled,
                    "REJECTED" => OrderState::Rejected,
                    "EXPIRED" => OrderState::Cancelled,
                    _ => OrderState::Submitted,
                };
                if cached_order.status != broker_status {
                    discrepancies.push(Discrepancy::OrderStatusMismatch {
                        order_id,
                        local_status: format!("{:?}", cached_order.status),
                        broker_status,
                    });
                }
            }
        }

        let has_critical_issues = !discrepancies.is_empty();

        // Block reconciliation if critical issues found
        if has_critical_issues {
            self.reconciliation_blocked = true;
        }

        Ok(DiscrepancyReport {
            discrepancies,
            has_critical_issues,
        })
    }

    /// Submit an order with optional sequence number for client order ID generation.
    ///
    /// If `sequence_number` is Some, a client order ID will be generated using
    /// `generate_client_order_id()`. If the order already has a `client_order_id`
    /// set, that will be used instead. Duplicate detection is performed before
    /// submission to prevent duplicate orders.
    pub fn submit_order_with_sequence(
        &self,
        order: &BrokerOrder,
        sequence_number: Option<u64>,
    ) -> Result<OrderId, BrokerError> {
        // Determine the client order ID to use
        let client_order_id = if let Some(existing_cid) = &order.client_order_id {
            Some(existing_cid.clone())
        } else if let Some(seq) = sequence_number {
            let cid_str = self.generate_client_order_id(seq);
            Some(ClientOrderId::new(cid_str)?)
        } else {
            None
        };

        // Check for duplicate in audit log if enabled
        if let (Some(audit_path), Some(seq)) = (&self.audit_log_path, sequence_number) {
            if check_audit_log_for_sequence(audit_path, seq).unwrap_or(false) {
                let cid_str = client_order_id.as_ref().map(|c| c.as_str()).unwrap_or("");
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

        // Check for duplicate if we have a client order ID
        if let Some(ref cid) = client_order_id {
            if self.check_duplicate_client_order_id(cid.as_str()) {
                // Log idempotency rejection if audit log is enabled
                if let Some(ref audit_path) = self.audit_log_path {
                    if let Some(seq) = sequence_number {
                        log_idempotency_rejection(
                            audit_path,
                            order.symbol,
                            seq,
                            cid.as_str(),
                            "duplicate client order ID in cache",
                        );
                    }
                }
                return Err(BrokerError::DuplicateOrder {
                    client_order_id: cid.as_str().to_string(),
                });
            }
        }

        // Create a modified order with the client order ID
        let order_with_cid = BrokerOrder {
            client_order_id,
            ..order.clone()
        };

        // Call the trait implementation
        let order_id = self.submit_order(&order_with_cid)?;

        // Log order submission to audit log if enabled
        if let (Some(audit_path), Some(seq), Some(cid)) =
            (&self.audit_log_path, sequence_number, &order_with_cid.client_order_id)
        {
            log_order_submitted(audit_path, order_id, order.symbol, seq, cid.as_str());
        }

        Ok(order_id)
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

        let mut positions = Vec::with_capacity(info.balances.len());
        for b in &info.balances {
            let free = parse_f64_or_warn(&b.free, "binance balance.free");
            let locked = parse_f64_or_warn(&b.locked, "binance balance.locked");
            let total = free + locked;
            if total <= 0.0 {
                continue;
            }
            let Some(sym) = Symbol::try_new(&b.asset) else {
                continue;
            };
            // Crypto positions are always positive (long); quantity in
            // smallest unit (satoshis for BTC, etc.).
            let qty = f64_to_fixed_checked(total, 1e8, "binance balance")?;
            positions.push(Position {
                symbol: sym,
                quantity: qty,
                avg_cost_cents: 0,     // Binance doesn't track avg cost
                market_value_cents: 0, // would need live prices
                unrealized_pnl_cents: 0,
            });
        }

        Ok(positions)
    }

    fn account(&self) -> Result<Account, BrokerError> {
        let client = self.require_client()?;
        let info = client.account_info()?;

        // Sum USDT-equivalent balance as a rough equity estimate.
        let usdt_balance: f64 = info
            .balances
            .iter()
            .filter(|b| b.asset == self.quote_asset)
            .map(|b| {
                let free = parse_f64_or_warn(&b.free, "binance balance.free");
                let locked = parse_f64_or_warn(&b.locked, "binance balance.locked");
                free + locked
            })
            .sum();

        let equity_cents = f64_cents_checked(usdt_balance, "binance equity")?;

        Ok(Account {
            equity_cents,
            buying_power_cents: equity_cents,
            cash_cents: equity_cents,
            gross_position_value_cents: 0,
        })
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        if self.reconciliation_blocked {
            return Err(BrokerError::Order(
                "Reconciliation blocked - manual review required".to_string(),
            ));
        }
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
            order.client_order_id.as_ref().map(|cid| cid.as_str()),
        )?;

        let order_id = OrderId(resp.order_id);
        self.cache_order_with_binance_id(
            order_id,
            order.symbol,
            i64::try_from(order.quantity)
                .map_err(|_| BrokerError::Order("order quantity exceeds i64".into()))?,
            order.side,
            resp.order_id.to_string(),
            order
                .client_order_id
                .as_ref()
                .map(|cid| cid.as_str().to_string()),
            Utc::now(),
        );

        Ok(order_id)
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

    fn open_orders(&self) -> Result<Vec<BrokerOrderStatus>, BrokerError> {
        // Binance open orders query requires GET /api/v3/openOrders endpoint.
        // For now return empty list. Full implementation requires
        // querying via the client and parsing the response.
        Ok(Vec::new())
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

        let bid = Self::parse_price_cents(&ticker.bid_price, "binance bid")?;
        let ask = Self::parse_price_cents(&ticker.ask_price, "binance ask")?;
        let last = (bid + ask) / 2; // Binance bookTicker doesn't have last; use mid

        Ok(Quote {
            symbol: *symbol,
            bid_cents: bid,
            ask_cents: ask,
            last_cents: last,
            volume: 0,
            timestamp: std::time::SystemTime::now(),
        })
    }
}
