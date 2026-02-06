//! Book snapshots for market data.

use crate::{OrderBook, Price, Quantity, Timestamp};

/// A snapshot of the order book at a point in time.
#[derive(Clone, Debug, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct BookSnapshot {
    /// Bid levels (highest price first)
    pub bids: Vec<LevelSnapshot>,
    /// Ask levels (lowest price first)
    pub asks: Vec<LevelSnapshot>,
    /// Timestamp when snapshot was taken
    pub timestamp: Timestamp,
}

impl BookSnapshot {
    /// Returns the best bid price, if any.
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.first().map(|l| l.price)
    }

    /// Returns the best ask price, if any.
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.first().map(|l| l.price)
    }

    /// Returns the spread (best ask - best bid), if both exist.
    pub fn spread(&self) -> Option<i64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.0 - bid.0),
            _ => None,
        }
    }

    /// Returns the mid price ((best bid + best ask) / 2), if both exist.
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some((bid.0 + ask.0) as f64 / 2.0),
            _ => None,
        }
    }

    /// Returns total bid quantity across all levels.
    pub fn total_bid_quantity(&self) -> Quantity {
        self.bids.iter().map(|l| l.quantity).sum()
    }

    /// Returns total ask quantity across all levels.
    pub fn total_ask_quantity(&self) -> Quantity {
        self.asks.iter().map(|l| l.quantity).sum()
    }

    /// Order book imbalance: `(bid_qty - ask_qty) / (bid_qty + ask_qty)`.
    ///
    /// Returns a value in `[-1.0, 1.0]`:
    /// - `+1.0` = all liquidity on bid side (buy pressure)
    /// - `-1.0` = all liquidity on ask side (sell pressure)
    /// - `0.0` = balanced
    ///
    /// Returns `None` if the book is empty on both sides.
    pub fn imbalance(&self) -> Option<f64> {
        let bid_qty = self.total_bid_quantity();
        let ask_qty = self.total_ask_quantity();
        let total = bid_qty + ask_qty;
        if total == 0 {
            return None;
        }
        Some((bid_qty as f64 - ask_qty as f64) / total as f64)
    }

    /// Volume-weighted midpoint price.
    ///
    /// Weighted by the inverse of each side's quantity at the top of book:
    /// `(ask_qty * bid_price + bid_qty * ask_price) / (bid_qty + ask_qty)`
    ///
    /// This gives a midpoint that leans toward the side with less liquidity,
    /// reflecting where the next trade is more likely to occur.
    ///
    /// Returns `None` if either side has no levels.
    pub fn weighted_mid(&self) -> Option<f64> {
        let bid = self.bids.first()?;
        let ask = self.asks.first()?;
        let total = bid.quantity + ask.quantity;
        if total == 0 {
            return None;
        }
        Some(
            (ask.quantity as f64 * bid.price.0 as f64 + bid.quantity as f64 * ask.price.0 as f64)
                / total as f64,
        )
    }
}

/// A snapshot of a single price level.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct LevelSnapshot {
    /// Price at this level
    pub price: Price,
    /// Total quantity at this level
    pub quantity: Quantity,
    /// Number of orders at this level
    pub order_count: usize,
}

impl OrderBook {
    /// Take a snapshot of the top N levels on each side.
    pub fn snapshot(&self, depth: usize) -> BookSnapshot {
        fn snapshot_levels(levels: &crate::PriceLevels, depth: usize) -> Vec<LevelSnapshot> {
            levels
                .iter_best_to_worst()
                .take(depth)
                .map(|(price, level)| LevelSnapshot {
                    price: *price,
                    quantity: level.total_quantity(),
                    order_count: level.order_count(),
                })
                .collect()
        }

        BookSnapshot {
            bids: snapshot_levels(self.bids(), depth),
            asks: snapshot_levels(self.asks(), depth),
            timestamp: self.peek_next_order_id().0,
        }
    }

    /// Take a full snapshot of all levels.
    pub fn full_snapshot(&self) -> BookSnapshot {
        self.snapshot(usize::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Side, TimeInForce};

    #[test]
    fn empty_snapshot() {
        let book = OrderBook::new();
        let snap = book.snapshot(10);

        assert!(snap.bids.is_empty());
        assert!(snap.asks.is_empty());
        assert_eq!(snap.best_bid(), None);
        assert_eq!(snap.best_ask(), None);
        assert_eq!(snap.spread(), None);
        assert_eq!(snap.mid_price(), None);
    }

    #[test]
    fn snapshot_with_orders() {
        let mut book = OrderBook::new();

        // Add some bids
        let b1 = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let b2 = book.create_order(Side::Buy, Price(100_00), 50, TimeInForce::GTC);
        let b3 = book.create_order(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
        book.add_order(b1);
        book.add_order(b2);
        book.add_order(b3);

        // Add some asks
        let a1 = book.create_order(Side::Sell, Price(101_00), 75, TimeInForce::GTC);
        let a2 = book.create_order(Side::Sell, Price(102_00), 150, TimeInForce::GTC);
        book.add_order(a1);
        book.add_order(a2);

        let snap = book.snapshot(10);

        // Check bids (best first = highest)
        assert_eq!(snap.bids.len(), 2);
        assert_eq!(snap.bids[0].price, Price(100_00));
        assert_eq!(snap.bids[0].quantity, 150); // 100 + 50
        assert_eq!(snap.bids[0].order_count, 2);
        assert_eq!(snap.bids[1].price, Price(99_00));
        assert_eq!(snap.bids[1].quantity, 200);

        // Check asks (best first = lowest)
        assert_eq!(snap.asks.len(), 2);
        assert_eq!(snap.asks[0].price, Price(101_00));
        assert_eq!(snap.asks[0].quantity, 75);
        assert_eq!(snap.asks[1].price, Price(102_00));

        // Check derived values
        assert_eq!(snap.best_bid(), Some(Price(100_00)));
        assert_eq!(snap.best_ask(), Some(Price(101_00)));
        assert_eq!(snap.spread(), Some(100)); // $1.00
        assert_eq!(snap.mid_price(), Some(100_50.0)); // $100.50

        assert_eq!(snap.total_bid_quantity(), 350);
        assert_eq!(snap.total_ask_quantity(), 225);
    }

    #[test]
    fn snapshot_depth_limit() {
        let mut book = OrderBook::new();

        // Add 5 bid levels
        for i in 0..5 {
            let order =
                book.create_order(Side::Buy, Price(100_00 - i * 100), 100, TimeInForce::GTC);
            book.add_order(order);
        }

        // Request only 3 levels
        let snap = book.snapshot(3);
        assert_eq!(snap.bids.len(), 3);
        assert_eq!(snap.bids[0].price, Price(100_00));
        assert_eq!(snap.bids[1].price, Price(99_00));
        assert_eq!(snap.bids[2].price, Price(98_00));
    }

    #[test]
    fn full_snapshot() {
        let mut book = OrderBook::new();

        for i in 0..10 {
            let order =
                book.create_order(Side::Buy, Price(100_00 - i * 100), 100, TimeInForce::GTC);
            book.add_order(order);
        }

        let snap = book.full_snapshot();
        assert_eq!(snap.bids.len(), 10);
    }

    // === Analytics tests ===

    #[test]
    fn imbalance_balanced() {
        let mut book = OrderBook::new();
        let b = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let a = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        book.add_order(b);
        book.add_order(a);

        let snap = book.snapshot(10);
        let imb = snap.imbalance().unwrap();
        assert!((imb).abs() < 1e-10); // balanced
    }

    #[test]
    fn imbalance_bid_heavy() {
        let mut book = OrderBook::new();
        let b = book.create_order(Side::Buy, Price(100_00), 300, TimeInForce::GTC);
        let a = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        book.add_order(b);
        book.add_order(a);

        let snap = book.snapshot(10);
        let imb = snap.imbalance().unwrap();
        // (300 - 100) / (300 + 100) = 0.5
        assert!((imb - 0.5).abs() < 1e-10);
    }

    #[test]
    fn imbalance_empty() {
        let book = OrderBook::new();
        let snap = book.snapshot(10);
        assert!(snap.imbalance().is_none());
    }

    #[test]
    fn weighted_mid_equal_qty() {
        let mut book = OrderBook::new();
        let b = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let a = book.create_order(Side::Sell, Price(102_00), 100, TimeInForce::GTC);
        book.add_order(b);
        book.add_order(a);

        let snap = book.snapshot(10);
        let wmid = snap.weighted_mid().unwrap();
        // Equal quantities → simple midpoint
        assert!((wmid - 101_00.0).abs() < 1e-10);
    }

    #[test]
    fn weighted_mid_skewed() {
        let mut book = OrderBook::new();
        let b = book.create_order(Side::Buy, Price(100_00), 300, TimeInForce::GTC);
        let a = book.create_order(Side::Sell, Price(102_00), 100, TimeInForce::GTC);
        book.add_order(b);
        book.add_order(a);

        let snap = book.snapshot(10);
        let wmid = snap.weighted_mid().unwrap();
        // More bid qty → weighted mid closer to ask
        // (100 * 10000 + 300 * 10200) / 400 = (1_000_000 + 3_060_000) / 400 = 10150
        assert!((wmid - 101_50.0).abs() < 1e-10);
    }

    #[test]
    fn weighted_mid_empty() {
        let book = OrderBook::new();
        let snap = book.snapshot(10);
        assert!(snap.weighted_mid().is_none());
    }
}
