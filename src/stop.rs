//! Stop orders: conditional orders triggered by trade price.
//!
//! Stop orders rest in a separate book and are triggered when the last
//! trade price reaches the stop price. Once triggered, they become
//! regular market or limit orders.

use std::collections::BTreeMap;

use rustc_hash::FxHashMap;

use crate::{OrderId, Price, Quantity, Side, TimeInForce, Timestamp};

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
}

/// Book of pending stop orders.
///
/// Maintains two price-indexed maps for efficient trigger lookups:
/// - Buy stops: trigger when `last_trade_price >= stop_price`
/// - Sell stops: trigger when `last_trade_price <= stop_price`
#[derive(Clone, Debug, Default)]
pub struct StopBook {
    /// Buy stop orders indexed by stop price.
    buy_stops: BTreeMap<Price, Vec<OrderId>>,
    /// Sell stop orders indexed by stop price.
    sell_stops: BTreeMap<Price, Vec<OrderId>>,
    /// All stop orders by ID.
    orders: FxHashMap<OrderId, StopOrder>,
}

impl StopBook {
    /// Create a new empty stop book.
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert a stop order into the book.
    pub fn insert(&mut self, order: StopOrder) {
        let id = order.id;
        let price = order.stop_price;
        let side = order.side;

        let map = match side {
            Side::Buy => &mut self.buy_stops,
            Side::Sell => &mut self.sell_stops,
        };
        map.entry(price).or_default().push(id);

        self.orders.insert(id, order);
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

    /// Clear triggered and cancelled stop orders from history.
    pub fn clear_history(&mut self) {
        self.orders
            .retain(|_, order| order.status == StopStatus::Pending);
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
        };
        book.insert(stop);

        let triggered = book.collect_triggered(Price(105_00));
        assert_eq!(triggered.len(), 1);
        assert_eq!(triggered[0].limit_price, Some(Price(106_00)));
    }
}
