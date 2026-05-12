//! Order submission, fill monitoring, rate limiting, and cancellation.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use ibapi::client::blocking::Client;
use ibapi::contracts::Contract;
use ibapi::orders::order_builder::limit_order;
#[cfg(not(feature = "strict-market-reject"))]
use ibapi::orders::order_builder::market_order;
use ibapi::orders::{Action as IbAction, CancelOrder, PlaceOrder};
use log::{debug, info, warn};

use crate::error::BrokerError;
use crate::types::*;

/// Deduplication key for order-status callbacks.
///
/// TWS (Trader Workstation) may send duplicate OrderStatus callbacks for the
/// same fill event due to network retries, internal TWS state synchronization,
/// or other implementation details. Without deduplication, these duplicates
/// would cause position updates to be applied multiple times, leading to
/// incorrect position tracking.
///
/// We deduplicate based on the combination of:
/// - `order_id`: The broker-assigned order identifier
/// - `status`: The order status string (e.g., "Filled", "Submitted")
/// - `filled_quantity`: The quantity filled (rounded to integer for hashing)
///
/// This ensures that each unique fill event is processed exactly once,
/// regardless of how many duplicate callbacks TWS sends.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct OrderCallbackKey {
    pub order_id: i32,
    pub status: String,
    pub filled_quantity: i64, // Converted from f64 for hashing
}

/// Deduplication cache type for order-status callbacks.
///
/// The cache maps each unique callback to the timestamp when it was first
/// seen. Entries are automatically cleaned up after a TTL (time-to-live) of
/// 5 minutes to prevent unbounded memory growth.
///
/// The TTL of 5 minutes is chosen to be:
/// - Long enough to cover the typical window for duplicate callbacks
/// - Short enough to prevent the cache from growing indefinitely
/// - Sufficient for normal order execution flows (orders typically fill within seconds)
pub type CallbackDedupCache = Mutex<HashMap<OrderCallbackKey, Instant>>;

/// Result of a single order execution.
#[derive(Debug, Clone)]
pub struct OrderResult {
    pub symbol: nanobook::Symbol,
    pub order_id: i32,
    pub filled_shares: i64,
    pub avg_fill_price: f64,
    pub commission: f64,
    pub status: OrderOutcome,
}

/// How an order ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderOutcome {
    Filled,
    PartialFill,
    Cancelled,
    Failed,
}

/// Encode a `BrokerOrder` into the `(limit_price_f64, qty_f64)` pair used by
/// the quote-bounded fallback path.
///
/// Investigation note: `ibapi` 2.7 and 2.11 both expose true market orders via
/// `order_builder::market_order` (`order_type = "MKT"`), and
/// `OrderType::Market` does not require a limit price. Live IBKR market
/// submissions therefore use true market orders. This encoder remains the
/// bounded aggressive-limit helper for callers that explicitly choose a
/// quote-bounded fallback.
///
/// # Errors
/// - `BrokerError::NoQuoteForMarketOrder` if a market order is encoded without
///   a cached NBBO quote.
/// - `BrokerError::MarketOrderRejected` if `strict-market-reject` is enabled.
pub fn encode_order(
    order: &BrokerOrder,
    best_quote: Option<&BestQuote>,
) -> Result<(f64, f64), BrokerError> {
    #[cfg(feature = "strict-market-reject")]
    let _ = best_quote;

    let quantity = order.quantity as f64;
    match order.order_type {
        #[cfg(feature = "strict-market-reject")]
        BrokerOrderType::Market => Err(BrokerError::MarketOrderRejected),

        #[cfg(not(feature = "strict-market-reject"))]
        BrokerOrderType::Market => {
            let quote = best_quote.ok_or_else(|| BrokerError::NoQuoteForMarketOrder {
                symbol: order.symbol.to_string(),
            })?;
            const SLIP_BPS: f64 = 50.0;
            let bps = SLIP_BPS / 10_000.0;
            let price = match order.side {
                BrokerSide::Buy => (quote.ask_cents as f64 / 100.0) * (1.0 + bps),
                BrokerSide::Sell => (quote.bid_cents as f64 / 100.0) * (1.0 - bps),
            };
            Ok((price, quantity))
        }

        BrokerOrderType::Limit(price) => Ok((price.0 as f64 / 100.0, quantity)),
    }
}

/// Submit an order via the IBKR API. Returns the broker-assigned OrderId.
pub fn submit_order(
    client: &Client,
    order: &BrokerOrder,
    best_quote: Option<&BestQuote>,
) -> Result<OrderId, BrokerError> {
    let contract = Contract::stock(order.symbol.as_str()).build();

    let ib_action = match order.side {
        BrokerSide::Buy => IbAction::Buy,
        BrokerSide::Sell => IbAction::Sell,
    };

    let mut ib_order = match order.order_type {
        #[cfg(feature = "strict-market-reject")]
        BrokerOrderType::Market => return Err(BrokerError::MarketOrderRejected),

        #[cfg(not(feature = "strict-market-reject"))]
        BrokerOrderType::Market => market_order(ib_action, order.quantity as f64),

        BrokerOrderType::Limit(_) => {
            let (limit_price, quantity) = encode_order(order, best_quote)?;
            limit_order(ib_action, quantity, limit_price)
        }
    };

    if let Some(cid) = &order.client_order_id {
        ib_order.order_ref = cid.as_str().to_string();
    }

    let order_id = client
        .next_valid_order_id()
        .map_err(|e| BrokerError::Order(format!("failed to get order id: {e}")))?;

    match order.order_type {
        BrokerOrderType::Market => info!(
            "Submitting: {:?} {} {} @ MKT (id={})",
            order.side, order.quantity, order.symbol, order_id
        ),
        BrokerOrderType::Limit(price) => info!(
            "Submitting: {:?} {} {} @ ${:.2} (id={})",
            order.side,
            order.quantity,
            order.symbol,
            price.0 as f64 / 100.0,
            order_id
        ),
    }

    let _subscription = client
        .place_order(order_id, &contract, &ib_order)
        .map_err(|e| BrokerError::Order(format!("failed to place order {order_id}: {e}")))?;

    Ok(OrderId(order_id as u64))
}

/// Execute a rebalance-style order: submit limit, poll for fill, cancel on timeout.
///
/// This is the higher-level function used by the rebalancer for order-by-order execution.
///
/// # Deduplication
///
/// The `dedup_cache` parameter enables deduplication of order-status callbacks to
/// handle TWS duplicate callbacks. When provided, the function will:
/// - Check each OrderStatus callback against the cache
/// - Skip processing of duplicate callbacks (logged at debug level)
/// - Record new callbacks in the cache with a timestamp
/// - Automatically clean up expired entries (TTL: 5 minutes) on each check
///
/// When `None`, no deduplication is performed. This is useful for:
/// - Testing scenarios where duplicates are not expected
/// - Environments where TWS behavior is known to be stable
///
/// # Why Deduplication is Needed
///
/// TWS (Trader Workstation) may send duplicate OrderStatus callbacks for the same
/// fill event. Without deduplication, these duplicates would cause:
/// - Position updates to be applied multiple times
/// - Incorrect position tracking
/// - Potential double-counting of fills
///
/// The deduplication logic ensures that each unique fill event (identified by
/// order_id + status + filled_quantity) is processed exactly once.
pub fn execute_limit_order(
    client: &Client,
    symbol: nanobook::Symbol,
    side: BrokerSide,
    shares: i64,
    limit_price_cents: i64,
    client_order_id: Option<&ClientOrderId>,
    timeout: Duration,
    dedup_cache: Option<&CallbackDedupCache>,
) -> Result<OrderResult, BrokerError> {
    let contract = Contract::stock(symbol.as_str()).build();

    let ib_action = match side {
        BrokerSide::Buy => IbAction::Buy,
        BrokerSide::Sell => IbAction::Sell,
    };

    let limit_price = limit_price_cents as f64 / 100.0;
    let quantity = shares as f64;

    let mut ib_order = limit_order(ib_action, quantity, limit_price);
    if let Some(cid) = client_order_id {
        ib_order.order_ref = cid.as_str().to_string();
    }

    let order_id = client
        .next_valid_order_id()
        .map_err(|e| BrokerError::Order(format!("failed to get order id: {e}")))?;

    info!(
        "Submitting: {:?} {} {} @ ${:.2} (id={})",
        side, shares, symbol, limit_price, order_id
    );

    let subscription = client
        .place_order(order_id, &contract, &ib_order)
        .map_err(|e| BrokerError::Order(format!("failed to place order {order_id}: {e}")))?;

    let start = Instant::now();
    let mut filled = 0.0_f64;
    let mut avg_price = 0.0_f64;
    let mut commission = 0.0_f64;
    let mut final_status = OrderOutcome::Failed;

    for response in subscription {
        if start.elapsed() > timeout {
            warn!("Order {order_id} timed out after {}s", timeout.as_secs());
            cancel_order(client, order_id);
            final_status = if filled > 0.0 {
                OrderOutcome::PartialFill
            } else {
                OrderOutcome::Cancelled
            };
            break;
        }

        match response {
            PlaceOrder::OrderStatus(status) => {
                // Check for duplicate callbacks if dedup cache is provided
                if let Some(cache) = dedup_cache {
                    let key = OrderCallbackKey {
                        order_id,
                        status: status.status.clone(),
                        filled_quantity: status.filled.round() as i64,
                    };

                    let is_duplicate = {
                        let mut cache_guard = cache
                            .lock()
                            .map_err(|_| BrokerError::Other("dedup cache poisoned".into()))?;

                        // Clean up expired entries (TTL: 5 minutes)
                        let ttl = Duration::from_secs(300);
                        cache_guard.retain(|_, timestamp| timestamp.elapsed() < ttl);

                        // Check if this is a duplicate
                        let duplicate = cache_guard.contains_key(&key);

                        // Record this callback if not a duplicate
                        if !duplicate {
                            cache_guard.insert(key, Instant::now());
                        }

                        duplicate
                    };

                    if is_duplicate {
                        debug!(
                            "Skipping duplicate OrderStatus for order {}: status={}, filled={}",
                            order_id, status.status, status.filled
                        );
                        continue; // Skip processing this duplicate
                    }
                }

                debug!(
                    "Order {order_id} status: {} filled={} remaining={}",
                    status.status, status.filled, status.remaining
                );
                filled = status.filled;
                avg_price = status.average_fill_price;

                if status.status == "Filled" {
                    final_status = OrderOutcome::Filled;
                    break;
                } else if status.status == "Cancelled" {
                    final_status = if filled > 0.0 {
                        OrderOutcome::PartialFill
                    } else {
                        OrderOutcome::Cancelled
                    };
                    break;
                }
            }
            PlaceOrder::ExecutionData(exec) => {
                debug!(
                    "Execution: {} shares @ ${:.2}",
                    exec.execution.shares, exec.execution.price
                );
            }
            PlaceOrder::CommissionReport(comm) => {
                commission = comm.commission;
                debug!("Commission: ${:.4}", commission);
            }
            PlaceOrder::Message(notice) if notice.code < 0 || notice.code >= 2000 => {
                warn!("Order {order_id} error {}: {}", notice.code, notice.message);
            }
            _ => {}
        }
    }

    let result = OrderResult {
        symbol,
        order_id,
        filled_shares: filled as i64,
        avg_fill_price: avg_price,
        commission,
        status: final_status,
    };

    info!(
        "Order {order_id}: {:?} -- filled {} @ ${:.2}",
        final_status, result.filled_shares, avg_price
    );

    Ok(result)
}

/// Cancel an order by ID.
pub fn cancel_order(client: &Client, order_id: i32) {
    info!("Cancelling order {order_id}");
    match client.cancel_order(order_id, "") {
        Ok(subscription) => {
            for response in subscription {
                match response {
                    CancelOrder::OrderStatus(s) => {
                        debug!("Cancel status for {order_id}: {}", s.status);
                        if s.status == "Cancelled" {
                            break;
                        }
                    }
                    CancelOrder::Notice(notice) => {
                        debug!("Cancel notice for {order_id}: {}", notice.message);
                    }
                }
            }
        }
        Err(e) => {
            warn!("Failed to cancel order {order_id}: {e}");
        }
    }
}

/// Sleep for the rate-limit interval between orders.
pub fn rate_limit_delay(interval_ms: u64) {
    if interval_ms > 0 {
        thread::sleep(Duration::from_millis(interval_ms));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_cache_detects_duplicates() {
        let cache = CallbackDedupCache::default();

        let order_id = 12345;
        let status = "Filled";
        let filled_qty: f64 = 100.0;

        // First callback should not be a duplicate
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let mut cache_guard = cache.lock().unwrap();
            assert!(!cache_guard.contains_key(&key));
            cache_guard.insert(key, Instant::now());
        }

        // Second identical callback should be detected as duplicate
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(cache_guard.contains_key(&key));
        }

        // Different filled quantity should not be a duplicate
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: (filled_qty + 50.0).round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(!cache_guard.contains_key(&key));
        }

        // Different status should not be a duplicate
        {
            let key = OrderCallbackKey {
                order_id,
                status: "Submitted".to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(!cache_guard.contains_key(&key));
        }
    }

    #[test]
    fn test_dedup_cache_ttl_cleanup() {
        let cache = CallbackDedupCache::default();

        let order_id = 12345;
        let status = "Filled";
        let filled_qty: f64 = 100.0;

        // Insert an entry
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.insert(key, Instant::now());
        }

        // Entry should exist immediately
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(cache_guard.contains_key(&key));
        }

        // Simulate TTL cleanup (remove entries older than TTL)
        let ttl = Duration::from_secs(300);
        {
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.retain(|_, timestamp| timestamp.elapsed() < ttl);
        }

        // Entry should still exist (not expired yet)
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(cache_guard.contains_key(&key));
        }

        // Manually expire the entry by setting old timestamp
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.insert(key, Instant::now() - Duration::from_secs(301));
        }

        // Run TTL cleanup
        {
            let mut cache_guard = cache.lock().unwrap();
            cache_guard.retain(|_, timestamp| timestamp.elapsed() < ttl);
        }

        // Entry should be removed after TTL
        {
            let key = OrderCallbackKey {
                order_id,
                status: status.to_string(),
                filled_quantity: filled_qty.round() as i64,
            };
            let cache_guard = cache.lock().unwrap();
            assert!(!cache_guard.contains_key(&key));
        }
    }
}
