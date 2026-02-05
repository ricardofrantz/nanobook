//! Book snapshots for market data.

use crate::{OrderBook, Price, Quantity, Timestamp};

/// A snapshot of the order book at a point in time.
#[derive(Clone, Debug, Default)]
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
}

/// A snapshot of a single price level.
#[derive(Clone, Debug)]
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
        let bids: Vec<_> = self
            .bids()
            .iter_best_to_worst()
            .take(depth)
            .map(|(price, level)| LevelSnapshot {
                price: *price,
                quantity: level.total_quantity(),
                order_count: level.order_count(),
            })
            .collect();

        let asks: Vec<_> = self
            .asks()
            .iter_best_to_worst()
            .take(depth)
            .map(|(price, level)| LevelSnapshot {
                price: *price,
                quantity: level.total_quantity(),
                order_count: level.order_count(),
            })
            .collect();

        BookSnapshot {
            bids,
            asks,
            timestamp: self.peek_next_order_id().0, // Use current counter as proxy
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
}
