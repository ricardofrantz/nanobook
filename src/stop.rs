//! Stop orders: conditional orders triggered by trade price.
//!
//! Stop orders rest in a separate book and are triggered when the last
//! trade price reaches the stop price. Once triggered, they become
//! regular market or limit orders.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::{OrderId, Price, Quantity, Side, TimeInForce, Timestamp};

/// Method used to compute the trailing stop offset.
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum TrailMethod {
    /// Fixed offset in cents (e.g., 200 = $2.00 trailing distance).
    Fixed(i64),
    /// Percentage of the watermark price (e.g., 0.02 = 2% trailing distance).
    Percentage(f64),
    /// ATR-based: `multiplier × ATR(period)`.
    ///
    /// ATR is computed internally from trade price changes.
    /// `period` is the lookback window size for the moving average.
    Atr {
        multiplier: f64,
        period: usize,
    },
}

/// Status of a stop order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum StopStatus {
    /// Waiting for trigger price to be reached.
    Pending,
    /// Stop price was reached; order has been submitted to the book.
    Triggered,
    /// Cancelled before being triggered.
    Cancelled,
}

/// A stop order waiting to be triggered.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct StopOrder {
    /// Unique identifier (shared with regular order ID space).
    pub id: OrderId,
    /// Buy or sell.
    pub side: Side,
    /// Price at which the stop triggers.
    pub stop_price: Price,
    /// Limit price for stop-limit orders (None = stop-market).
    pub limit_price: Option<Price>,
    /// Quantity to submit when triggered.
    pub quantity: Quantity,
    /// Time-in-force for the resulting order.
    pub time_in_force: TimeInForce,
    /// When the stop order was submitted.
    pub timestamp: Timestamp,
    /// Current status.
    pub status: StopStatus,
    /// Trailing stop method (None = regular stop).
    pub trail_method: Option<TrailMethod>,
    /// Watermark: best price seen (high for sell trailing, low for buy trailing).
    pub watermark: Option<Price>,
}

/// Book of pending stop orders.
///
/// Maintains two price-indexed maps for efficient trigger lookups:
/// - Buy stops: trigger when `last_trade_price >= stop_price`
/// - Sell stops: trigger when `last_trade_price <= stop_price`
///
/// Also maintains a rolling window of trade price changes for ATR computation.
#[derive(Clone, Debug, Default)]
pub struct StopBook {
    /// Buy stop orders indexed by stop price.
    buy_stops: BTreeMap<Price, Vec<OrderId>>,
    /// Sell stop orders indexed by stop price.
    sell_stops: BTreeMap<Price, Vec<OrderId>>,
    /// All stop orders by ID.
    orders: FxHashMap<OrderId, StopOrder>,
    /// IDs of trailing stop orders (for efficient update iteration).
    trailing_ids: Vec<OrderId>,
    /// Rolling window of absolute price changes for ATR computation.
    price_changes: Vec<i64>,
    /// Last trade price seen (for computing price changes).
    last_price: Option<Price>,
}

impl StopBook {
    /// Maximum price change history for ATR computation.
    /// Caps memory usage while allowing reasonable lookback periods.
    const MAX_PRICE_HISTORY: usize = 10_000;

    /// Create a new empty stop book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a stop order into the book.
    pub fn insert(&mut self, order: StopOrder) {
        let id = order.id;
        let price = order.stop_price;
        let side = order.side;
        let is_trailing = order.trail_method.is_some();

        let map = match side {
            Side::Buy => &mut self.buy_stops,
            Side::Sell => &mut self.sell_stops,
        };
        map.entry(price).or_default().push(id);

        self.orders.insert(id, order);

        if is_trailing {
            self.trailing_ids.push(id);
        }
    }

    /// Cancel a stop order. Returns true if the order was pending.
    pub fn cancel(&mut self, order_id: OrderId) -> bool {
        let order = match self.orders.get_mut(&order_id) {
            Some(o) if o.status == StopStatus::Pending => o,
            _ => return false,
        };

        let price = order.stop_price;
        let side = order.side;
        order.status = StopStatus::Cancelled;

        let map = match side {
            Side::Buy => &mut self.buy_stops,
            Side::Sell => &mut self.sell_stops,
        };
        if let Some(ids) = map.get_mut(&price) {
            ids.retain(|id| *id != order_id);
            if ids.is_empty() {
                map.remove(&price);
            }
        }

        // Remove from trailing index if it was a trailing stop
        self.trailing_ids.retain(|id| *id != order_id);

        true
    }

    /// Collect all stop orders triggered by a trade at the given price.
    ///
    /// Triggered orders are removed from the pending book and returned
    /// sorted by timestamp (FIFO).
    pub fn collect_triggered(&mut self, trade_price: Price) -> Vec<StopOrder> {
        let mut triggered = Vec::new();

        // Buy stops trigger when trade_price >= stop_price
        // Collect all buy stops with stop_price <= trade_price
        let buy_keys: Vec<Price> = self
            .buy_stops
            .range(..=trade_price)
            .map(|(k, _)| *k)
            .collect();
        for key in buy_keys {
            if let Some(ids) = self.buy_stops.remove(&key) {
                for id in ids {
                    if let Some(order) = self.orders.get_mut(&id) {
                        if order.status == StopStatus::Pending {
                            order.status = StopStatus::Triggered;
                            triggered.push(order.clone());
                        }
                    }
                }
            }
        }

        // Sell stops trigger when trade_price <= stop_price
        // Collect all sell stops with stop_price >= trade_price
        let sell_keys: Vec<Price> = self
            .sell_stops
            .range(trade_price..)
            .map(|(k, _)| *k)
            .collect();
        for key in sell_keys {
            if let Some(ids) = self.sell_stops.remove(&key) {
                for id in ids {
                    if let Some(order) = self.orders.get_mut(&id) {
                        if order.status == StopStatus::Pending {
                            order.status = StopStatus::Triggered;
                            triggered.push(order.clone());
                        }
                    }
                }
            }
        }

        // Sort by timestamp for deterministic FIFO ordering
        triggered.sort_by_key(|o| o.timestamp);

        // Prune triggered IDs from trailing index to avoid scanning them
        if !triggered.is_empty() {
            self.trailing_ids
                .retain(|id| self.orders.get(id).is_some_and(|o| o.status == StopStatus::Pending));
        }

        triggered
    }

    /// Get a stop order by ID.
    pub fn get(&self, order_id: OrderId) -> Option<&StopOrder> {
        self.orders.get(&order_id)
    }

    /// Returns true if there are no pending stop orders.
    pub fn is_empty(&self) -> bool {
        self.buy_stops.is_empty() && self.sell_stops.is_empty()
    }

    /// Returns the number of pending stop orders.
    pub fn pending_count(&self) -> usize {
        self.buy_stops.values().map(|v| v.len()).sum::<usize>()
            + self.sell_stops.values().map(|v| v.len()).sum::<usize>()
    }

    /// Record a trade price for ATR computation and update trailing stops.
    ///
    /// Call this BEFORE `collect_triggered()` so trailing stop prices
    /// are adjusted before checking triggers.
    pub fn update_trailing_stops(&mut self, trade_price: Price) {
        // Update price change history for ATR
        if let Some(prev) = self.last_price {
            let change = (trade_price.0 - prev.0).abs();
            self.price_changes.push(change);
            // Cap history to prevent unbounded growth
            if self.price_changes.len() > Self::MAX_PRICE_HISTORY {
                let excess = self.price_changes.len() - Self::MAX_PRICE_HISTORY;
                self.price_changes.drain(..excess);
            }
        }
        self.last_price = Some(trade_price);

        // Snapshot pending trailing IDs to avoid borrow conflicts
        let trailing_ids = self.trailing_ids.to_vec();

        for id in trailing_ids {
            let order = match self.orders.get(&id) {
                Some(o) if o.status == StopStatus::Pending && o.trail_method.is_some() => o,
                _ => continue,
            };

            let side = order.side;
            let trail_method = order.trail_method.clone().unwrap();

            // Update watermark
            let old_watermark = order.watermark.unwrap_or(trade_price);
            let new_watermark = match side {
                Side::Sell => Price(old_watermark.0.max(trade_price.0)),
                Side::Buy => Price(old_watermark.0.min(trade_price.0)),
            };

            // Compute trailing offset
            let offset = match &trail_method {
                TrailMethod::Fixed(cents) => *cents,
                TrailMethod::Percentage(pct) => (new_watermark.0 as f64 * pct) as i64,
                TrailMethod::Atr { multiplier, period } => {
                    self.compute_atr_offset(*multiplier, *period)
                }
            };

            if offset <= 0 {
                // ATR not ready yet -- update watermark only
                self.orders.get_mut(&id).unwrap().watermark = Some(new_watermark);
                continue;
            }

            // Compute new stop price
            let new_stop = match side {
                Side::Sell => Price(new_watermark.0 - offset),
                Side::Buy => Price(new_watermark.0 + offset),
            };

            let old_stop = order.stop_price;

            // Only move the stop in the favorable direction (tighter protection).
            // This applies even on the first update — if the market moved adversely,
            // the initial stop_price is preserved so the user's intended protection
            // level is not widened.
            let should_update = match side {
                Side::Sell => new_stop > old_stop,
                Side::Buy => new_stop < old_stop,
            };

            if should_update {
                // Re-index in BTreeMap
                let map = match side {
                    Side::Buy => &mut self.buy_stops,
                    Side::Sell => &mut self.sell_stops,
                };

                if let Some(ids) = map.get_mut(&old_stop) {
                    ids.retain(|oid| *oid != id);
                    if ids.is_empty() {
                        map.remove(&old_stop);
                    }
                }

                map.entry(new_stop).or_default().push(id);
            }

            // Always update watermark and (conditionally) stop price
            let order = self.orders.get_mut(&id).unwrap();
            if should_update {
                order.stop_price = new_stop;
            }
            order.watermark = Some(new_watermark);
        }
    }

    /// Compute ATR-based offset in cents.
    fn compute_atr_offset(&self, multiplier: f64, period: usize) -> i64 {
        if self.price_changes.is_empty() || period == 0 {
            return 0;
        }
        let window = if self.price_changes.len() >= period {
            &self.price_changes[self.price_changes.len() - period..]
        } else {
            &self.price_changes
        };
        let atr = window.iter().sum::<i64>() as f64 / window.len() as f64;
        (atr * multiplier) as i64
    }

    /// Clear triggered and cancelled stop orders from history.
    pub fn clear_history(&mut self) {
        self.orders
            .retain(|_, order| order.status == StopStatus::Pending);
        self.trailing_ids
            .retain(|id| self.orders.contains_key(id));
    }

    /// Check if a stop order exists (pending only).
    pub fn contains_pending(&self, order_id: OrderId) -> bool {
        self.orders
            .get(&order_id)
            .is_some_and(|o| o.status == StopStatus::Pending)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_stop(id: u64, side: Side, stop_price: i64, qty: u64, ts: u64) -> StopOrder {
        StopOrder {
            id: OrderId(id),
            side,
            stop_price: Price(stop_price),
            limit_price: None,
            quantity: qty,
            time_in_force: TimeInForce::GTC,
            timestamp: ts,
            status: StopStatus::Pending,
            trail_method: None,
            watermark: None,
        }
    }

    fn make_trailing_stop(
        id: u64,
        side: Side,
        stop_price: i64,
        qty: u64,
        ts: u64,
        method: TrailMethod,
    ) -> StopOrder {
        StopOrder {
            id: OrderId(id),
            side,
            stop_price: Price(stop_price),
            limit_price: None,
            quantity: qty,
            time_in_force: TimeInForce::GTC,
            timestamp: ts,
            status: StopStatus::Pending,
            trail_method: Some(method),
            watermark: None,
        }
    }

    #[test]
    fn insert_and_get() {
        let mut book = StopBook::new();
        let stop = make_stop(1, Side::Buy, 100_00, 100, 1);
        book.insert(stop);

        assert_eq!(book.pending_count(), 1);
        assert!(!book.is_empty());

        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.stop_price, Price(100_00));
        assert_eq!(order.status, StopStatus::Pending);
    }

    #[test]
    fn cancel_pending() {
        let mut book = StopBook::new();
        book.insert(make_stop(1, Side::Buy, 100_00, 100, 1));

        assert!(book.cancel(OrderId(1)));
        assert_eq!(book.pending_count(), 0);
        assert!(book.is_empty());

        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.status, StopStatus::Cancelled);
    }

    #[test]
    fn cancel_nonexistent_returns_false() {
        let mut book = StopBook::new();
        assert!(!book.cancel(OrderId(999)));
    }

    #[test]
    fn trigger_buy_stop() {
        let mut book = StopBook::new();
        // Buy stop at 105: triggers when price >= 105
        book.insert(make_stop(1, Side::Buy, 105_00, 100, 1));

        // Price at 104 — no trigger
        let triggered = book.collect_triggered(Price(104_00));
        assert!(triggered.is_empty());
        assert_eq!(book.pending_count(), 1);

        // Price at 105 — triggers
        let triggered = book.collect_triggered(Price(105_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, OrderId(1));
        assert_eq!(book.pending_count(), 0);
    }

    #[test]
    fn trigger_sell_stop() {
        let mut book = StopBook::new();
        // Sell stop at 95: triggers when price <= 95
        book.insert(make_stop(1, Side::Sell, 95_00, 100, 1));

        // Price at 96 — no trigger
        let triggered = book.collect_triggered(Price(96_00));
        assert!(triggered.is_empty());

        // Price at 95 — triggers
        let triggered = book.collect_triggered(Price(95_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, OrderId(1));
    }

    #[test]
    fn trigger_multiple_at_same_price() {
        let mut book = StopBook::new();
        book.insert(make_stop(1, Side::Buy, 100_00, 50, 1));
        book.insert(make_stop(2, Side::Buy, 100_00, 75, 2));

        let triggered = book.collect_triggered(Price(100_00));
        assert_eq!(triggered.len(), 2);
        // Sorted by timestamp
        assert_eq!(triggered[0].id, OrderId(1));
        assert_eq!(triggered[1].id, OrderId(2));
    }

    #[test]
    fn trigger_across_price_levels() {
        let mut book = StopBook::new();
        book.insert(make_stop(1, Side::Buy, 100_00, 50, 1));
        book.insert(make_stop(2, Side::Buy, 99_00, 75, 2));
        book.insert(make_stop(3, Side::Buy, 101_00, 25, 3));

        // Trade at 100 triggers stops at 99 and 100 but not 101
        let triggered = book.collect_triggered(Price(100_00));
        assert_eq!(triggered.len(), 2);
        assert_eq!(triggered[0].id, OrderId(1));
        assert_eq!(triggered[1].id, OrderId(2));

        assert_eq!(book.pending_count(), 1);
    }

    #[test]
    fn fifo_ordering_across_sides() {
        let mut book = StopBook::new();
        // Buy stop submitted at t=1
        book.insert(make_stop(1, Side::Buy, 100_00, 50, 1));
        // Sell stop submitted at t=2
        book.insert(make_stop(2, Side::Sell, 100_00, 50, 2));
        // Buy stop submitted at t=3
        book.insert(make_stop(3, Side::Buy, 99_00, 50, 3));

        // Trade at 100 triggers all three
        let triggered = book.collect_triggered(Price(100_00));
        assert_eq!(triggered.len(), 3);
        // Sorted by timestamp regardless of side
        assert_eq!(triggered[0].id, OrderId(1));
        assert_eq!(triggered[1].id, OrderId(2));
        assert_eq!(triggered[2].id, OrderId(3));
    }

    #[test]
    fn clear_history() {
        let mut book = StopBook::new();
        book.insert(make_stop(1, Side::Buy, 100_00, 50, 1));
        book.insert(make_stop(2, Side::Buy, 100_00, 75, 2));

        // Trigger one
        book.collect_triggered(Price(100_00));

        // Add a new pending one
        book.insert(make_stop(3, Side::Buy, 105_00, 100, 3));

        book.clear_history();

        // Triggered orders removed, pending preserved
        assert!(book.get(OrderId(1)).is_none());
        assert!(book.get(OrderId(2)).is_none());
        assert!(book.get(OrderId(3)).is_some());
    }

    #[test]
    fn contains_pending() {
        let mut book = StopBook::new();
        book.insert(make_stop(1, Side::Buy, 100_00, 50, 1));

        assert!(book.contains_pending(OrderId(1)));
        assert!(!book.contains_pending(OrderId(999)));

        book.cancel(OrderId(1));
        assert!(!book.contains_pending(OrderId(1)));
    }

    #[test]
    fn stop_limit_order() {
        let mut book = StopBook::new();
        let stop = StopOrder {
            id: OrderId(1),
            side: Side::Buy,
            stop_price: Price(105_00),
            limit_price: Some(Price(106_00)),
            quantity: 100,
            time_in_force: TimeInForce::GTC,
            timestamp: 1,
            status: StopStatus::Pending,
            trail_method: None,
            watermark: None,
        };
        book.insert(stop);

        let triggered = book.collect_triggered(Price(105_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].limit_price, Some(Price(106_00)));
    }

    // === Trailing Stop Tests ===

    #[test]
    fn trailing_sell_stop_fixed() {
        let mut book = StopBook::new();
        // Sell trailing stop: initial stop at 98, trail by $2.00
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            98_00,
            100,
            1,
            TrailMethod::Fixed(2_00),
        ));

        // Price rises to 102 — watermark moves up, stop should follow to 100
        book.update_trailing_stops(Price(102_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(102_00)));
        assert_eq!(order.stop_price, Price(100_00));

        // Price rises further to 105 — stop should move to 103
        book.update_trailing_stops(Price(105_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(105_00)));
        assert_eq!(order.stop_price, Price(103_00));

        // Price drops to 104 — watermark stays, stop stays
        book.update_trailing_stops(Price(104_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(105_00)));
        assert_eq!(order.stop_price, Price(103_00));

        // Price drops to 103 — triggers the stop
        let triggered = book.collect_triggered(Price(103_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, OrderId(1));
    }

    #[test]
    fn trailing_buy_stop_fixed() {
        let mut book = StopBook::new();
        // Buy trailing stop: initial stop at 102, trail by $2.00
        book.insert(make_trailing_stop(
            1,
            Side::Buy,
            102_00,
            100,
            1,
            TrailMethod::Fixed(2_00),
        ));

        // Price drops to 97 — watermark moves down, stop follows to 99
        book.update_trailing_stops(Price(97_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(97_00)));
        assert_eq!(order.stop_price, Price(99_00));

        // Price drops further to 95 — stop should move to 97
        book.update_trailing_stops(Price(95_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(95_00)));
        assert_eq!(order.stop_price, Price(97_00));

        // Price rises to 96 — watermark stays at 95, stop stays at 97
        book.update_trailing_stops(Price(96_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(95_00)));
        assert_eq!(order.stop_price, Price(97_00));
    }

    #[test]
    fn trailing_stop_percentage() {
        let mut book = StopBook::new();
        // Sell trailing stop: trail by 2% of watermark
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            98_00,
            100,
            1,
            TrailMethod::Percentage(0.02),
        ));

        // Price rises to 200_00 — stop should be at 200 - 2% of 200 = 196
        book.update_trailing_stops(Price(200_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(200_00)));
        assert_eq!(order.stop_price, Price(196_00));
    }

    #[test]
    fn trailing_stop_atr() {
        let mut book = StopBook::new();
        // Sell trailing stop: trail by 2x ATR(3)
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            90_00,
            100,
            1,
            TrailMethod::Atr {
                multiplier: 2.0,
                period: 3,
            },
        ));

        // Feed price changes to build ATR history: 100, 102, 99, 101
        // Changes: |102-100|=200, |99-102|=300, |101-99|=200
        // ATR(3) = (200+300+200)/3 = 233 cents
        // Offset = 2.0 * 233 = 466 cents
        book.update_trailing_stops(Price(100_00));
        book.update_trailing_stops(Price(102_00));
        book.update_trailing_stops(Price(99_00));
        book.update_trailing_stops(Price(101_00));

        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(102_00))); // highest seen
        // ATR = (200+300+200)/3 ≈ 233, offset = 2*233 = 466
        // stop = 102_00 - 466 = 97_34
        // But stop only moves UP for sell, so check it moved from 90_00
        assert!(order.stop_price.0 > 90_00);
    }

    #[test]
    fn trailing_stop_reindexes_in_btreemap() {
        let mut book = StopBook::new();
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            98_00,
            100,
            1,
            TrailMethod::Fixed(2_00),
        ));

        // Verify initially indexed at 98_00
        assert!(book.sell_stops.contains_key(&Price(98_00)));

        // Price rises to 105 — stop should move to 103
        book.update_trailing_stops(Price(105_00));

        // Old price level should be removed, new one created
        assert!(!book.sell_stops.contains_key(&Price(98_00)));
        assert!(book.sell_stops.contains_key(&Price(103_00)));
    }

    #[test]
    fn trailing_stop_preserves_initial_on_adverse_move() {
        let mut book = StopBook::new();
        // Sell trailing stop: initial stop at 95, trail by $3
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            95_00,
            100,
            1,
            TrailMethod::Fixed(3_00),
        ));

        // First trade at 90 (adverse for sell trailing — market dropped below stop)
        // stop_from_watermark = 90 - 3 = 87, which is BELOW 95 (unfavorable)
        // The stop should NOT widen — it should stay at 95
        book.update_trailing_stops(Price(90_00));
        let order = book.get(OrderId(1)).unwrap();
        assert_eq!(order.watermark, Some(Price(90_00)));
        assert_eq!(order.stop_price, Price(95_00)); // preserved, not 87!
    }

    #[test]
    fn trailing_stop_cancel() {
        let mut book = StopBook::new();
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            98_00,
            100,
            1,
            TrailMethod::Fixed(2_00),
        ));

        assert!(book.cancel(OrderId(1)));
        assert_eq!(book.pending_count(), 0);

        // Update should be a no-op for cancelled trailing stops
        book.update_trailing_stops(Price(110_00));
    }

    #[test]
    fn trailing_and_regular_stops_coexist() {
        let mut book = StopBook::new();
        // Regular sell stop at 95
        book.insert(make_stop(1, Side::Sell, 95_00, 50, 1));
        // Trailing sell stop starting at 98
        book.insert(make_trailing_stop(
            2,
            Side::Sell,
            98_00,
            100,
            2,
            TrailMethod::Fixed(2_00),
        ));

        // Price rises to 105 — trailing moves to 103, regular stays at 95
        book.update_trailing_stops(Price(105_00));

        let regular = book.get(OrderId(1)).unwrap();
        assert_eq!(regular.stop_price, Price(95_00));

        let trailing = book.get(OrderId(2)).unwrap();
        assert_eq!(trailing.stop_price, Price(103_00));

        // Trigger at 103 — only trailing triggers
        let triggered = book.collect_triggered(Price(103_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].id, OrderId(2));
    }

    #[test]
    fn clear_history_cleans_trailing_ids() {
        let mut book = StopBook::new();
        book.insert(make_trailing_stop(
            1,
            Side::Sell,
            98_00,
            100,
            1,
            TrailMethod::Fixed(2_00),
        ));
        book.insert(make_trailing_stop(
            2,
            Side::Sell,
            97_00,
            100,
            2,
            TrailMethod::Fixed(3_00),
        ));

        // Trigger one
        book.update_trailing_stops(Price(100_00));
        book.collect_triggered(Price(98_00));

        book.clear_history();
        // Only pending trailing stops remain in trailing_ids
        assert_eq!(book.trailing_ids.len(), 1);
    }
}
