//! PriceLevels: One side of the order book (bids or asks).
//!
//! Maintains a sorted collection of price levels with cached best price
//! for O(1) BBO (best bid/offer) queries.

use std::collections::BTreeMap;

use crate::{Level, OrderId, Price, Quantity, Side};

/// One side of the order book (all bids or all asks).
///
/// - **Bids**: Sorted high → low, best = highest price
/// - **Asks**: Sorted low → high, best = lowest price
///
/// The `BTreeMap` provides O(log n) insert/remove with sorted iteration.
/// Best price is cached for O(1) access.
#[derive(Clone, Debug)]
pub struct PriceLevels {
    /// Price levels, sorted by price
    levels: BTreeMap<Price, Level>,
    /// Cached best price for O(1) access
    best_price: Option<Price>,
    /// Which side this represents (determines "best" direction)
    side: Side,
}

impl PriceLevels {
    /// Create a new empty price levels collection for the given side.
    pub fn new(side: Side) -> Self {
        Self {
            levels: BTreeMap::new(),
            best_price: None,
            side,
        }
    }

    /// Returns which side this collection represents.
    #[inline]
    pub fn side(&self) -> Side {
        self.side
    }

    /// Returns true if there are no orders on this side.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.levels.is_empty()
    }

    /// Returns the number of distinct price levels.
    #[inline]
    pub fn level_count(&self) -> usize {
        self.levels.len()
    }

    /// Returns the best price (highest for bids, lowest for asks).
    ///
    /// O(1) - cached value.
    #[inline]
    pub fn best_price(&self) -> Option<Price> {
        self.best_price
    }

    /// Returns a reference to the best level.
    ///
    /// O(1) - uses cached best price.
    pub fn best_level(&self) -> Option<&Level> {
        self.best_price.and_then(|p| self.levels.get(&p))
    }

    /// Returns a mutable reference to the best level.
    ///
    /// O(1) - uses cached best price.
    pub fn best_level_mut(&mut self) -> Option<&mut Level> {
        self.best_price.and_then(|p| self.levels.get_mut(&p))
    }

    /// Returns a reference to the level at the given price, if it exists.
    pub fn get_level(&self, price: Price) -> Option<&Level> {
        self.levels.get(&price)
    }

    /// Returns a mutable reference to the level at the given price, if it exists.
    pub fn get_level_mut(&mut self, price: Price) -> Option<&mut Level> {
        self.levels.get_mut(&price)
    }

    /// Gets or creates a level at the given price.
    ///
    /// If the level is newly created, updates the best price cache if needed.
    pub fn get_or_create_level(&mut self, price: Price) -> &mut Level {
        // Check if we need to update best price before borrowing levels
        let is_new = !self.levels.contains_key(&price);

        if is_new {
            // Update best price cache before inserting
            self.update_best_price_after_insert(price);
            self.levels.insert(price, Level::new(price));
        }

        self.levels.get_mut(&price).unwrap()
    }

    /// Add an order at the given price.
    ///
    /// Creates the level if it doesn't exist.
    pub fn insert_order(&mut self, price: Price, order_id: OrderId, quantity: Quantity) {
        let level = self.get_or_create_level(price);
        level.push_back(order_id, quantity);
    }

    /// Remove an order from the given price level.
    ///
    /// Returns `true` if the order was found and removed.
    /// Removes the level entirely if it becomes empty.
    pub fn remove_order(&mut self, price: Price, order_id: OrderId, quantity: Quantity) -> bool {
        if let Some(level) = self.levels.get_mut(&price) {
            if level.remove(order_id, quantity) {
                if level.is_empty() {
                    self.remove_level(price);
                }
                return true;
            }
        }
        false
    }

    /// Remove a price level entirely.
    ///
    /// Updates the best price cache if the removed level was the best.
    pub fn remove_level(&mut self, price: Price) {
        if self.levels.remove(&price).is_some() {
            // Update best price if we removed it
            if self.best_price == Some(price) {
                self.recompute_best_price();
            }
        }
    }

    /// Remove the best level and return it.
    ///
    /// Useful when a level is fully consumed during matching.
    pub fn pop_best_level(&mut self) -> Option<Level> {
        let price = self.best_price?;
        let level = self.levels.remove(&price);
        self.recompute_best_price();
        level
    }

    /// Returns an iterator over levels from best to worst price.
    ///
    /// - Bids: highest to lowest
    /// - Asks: lowest to highest
    pub fn iter_best_to_worst(&self) -> impl Iterator<Item = (&Price, &Level)> {
        BestToWorstIter {
            side: self.side,
            forward: self.levels.iter(),
            backward: self.levels.iter().rev(),
        }
    }

    /// Returns the total quantity across all levels.
    pub fn total_quantity(&self) -> Quantity {
        self.levels.values().map(|l| l.total_quantity()).sum()
    }

    /// Returns the total quantity available at prices that would cross with the given price.
    ///
    /// For bids: quantity at prices >= given price
    /// For asks: quantity at prices <= given price
    pub fn quantity_at_or_better(&self, price: Price) -> Quantity {
        match self.side {
            Side::Buy => {
                // Bids: want prices >= given (higher is better for buyer)
                self.levels
                    .range(price..)
                    .map(|(_, l)| l.total_quantity())
                    .sum()
            }
            Side::Sell => {
                // Asks: want prices <= given (lower is better for seller's counterparty)
                self.levels
                    .range(..=price)
                    .map(|(_, l)| l.total_quantity())
                    .sum()
            }
        }
    }

    // === Private helpers ===

    /// Recompute best price from scratch (O(1) for BTreeMap).
    fn recompute_best_price(&mut self) {
        self.best_price = match self.side {
            Side::Buy => self.levels.keys().next_back().copied(), // Highest
            Side::Sell => self.levels.keys().next().copied(),     // Lowest
        };
    }

    /// Update best price after inserting a new level.
    fn update_best_price_after_insert(&mut self, new_price: Price) {
        match self.best_price {
            None => {
                self.best_price = Some(new_price);
            }
            Some(current_best) => {
                let is_better = match self.side {
                    Side::Buy => new_price > current_best, // Higher is better for bids
                    Side::Sell => new_price < current_best, // Lower is better for asks
                };
                if is_better {
                    self.best_price = Some(new_price);
                }
            }
        }
    }
}

/// Iterator that yields levels from best to worst price.
struct BestToWorstIter<'a, F, B>
where
    F: Iterator<Item = (&'a Price, &'a Level)>,
    B: Iterator<Item = (&'a Price, &'a Level)>,
{
    side: Side,
    forward: F,  // For asks (lowest first)
    backward: B, // For bids (highest first)
}

impl<'a, F, B> Iterator for BestToWorstIter<'a, F, B>
where
    F: Iterator<Item = (&'a Price, &'a Level)>,
    B: Iterator<Item = (&'a Price, &'a Level)>,
{
    type Item = (&'a Price, &'a Level);

    fn next(&mut self) -> Option<Self::Item> {
        match self.side {
            Side::Buy => self.backward.next(),
            Side::Sell => self.forward.next(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Bid side tests (best = highest) ===

    #[test]
    fn new_bids_is_empty() {
        let bids = PriceLevels::new(Side::Buy);

        assert!(bids.is_empty());
        assert_eq!(bids.level_count(), 0);
        assert_eq!(bids.best_price(), None);
        assert!(bids.best_level().is_none());
    }

    #[test]
    fn bids_best_is_highest() {
        let mut bids = PriceLevels::new(Side::Buy);

        bids.insert_order(Price(100_00), OrderId(1), 100);
        assert_eq!(bids.best_price(), Some(Price(100_00)));

        bids.insert_order(Price(99_00), OrderId(2), 100);
        assert_eq!(bids.best_price(), Some(Price(100_00))); // Still 100

        bids.insert_order(Price(101_00), OrderId(3), 100);
        assert_eq!(bids.best_price(), Some(Price(101_00))); // Now 101
    }

    #[test]
    fn bids_remove_best_updates_cache() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(99_00), OrderId(2), 100);
        bids.insert_order(Price(101_00), OrderId(3), 100);

        assert_eq!(bids.best_price(), Some(Price(101_00)));

        bids.remove_level(Price(101_00));
        assert_eq!(bids.best_price(), Some(Price(100_00)));

        bids.remove_level(Price(100_00));
        assert_eq!(bids.best_price(), Some(Price(99_00)));

        bids.remove_level(Price(99_00));
        assert_eq!(bids.best_price(), None);
    }

    // === Ask side tests (best = lowest) ===

    #[test]
    fn new_asks_is_empty() {
        let asks = PriceLevels::new(Side::Sell);

        assert!(asks.is_empty());
        assert_eq!(asks.best_price(), None);
    }

    #[test]
    fn asks_best_is_lowest() {
        let mut asks = PriceLevels::new(Side::Sell);

        asks.insert_order(Price(100_00), OrderId(1), 100);
        assert_eq!(asks.best_price(), Some(Price(100_00)));

        asks.insert_order(Price(101_00), OrderId(2), 100);
        assert_eq!(asks.best_price(), Some(Price(100_00))); // Still 100

        asks.insert_order(Price(99_00), OrderId(3), 100);
        assert_eq!(asks.best_price(), Some(Price(99_00))); // Now 99
    }

    #[test]
    fn asks_remove_best_updates_cache() {
        let mut asks = PriceLevels::new(Side::Sell);
        asks.insert_order(Price(100_00), OrderId(1), 100);
        asks.insert_order(Price(101_00), OrderId(2), 100);
        asks.insert_order(Price(99_00), OrderId(3), 100);

        assert_eq!(asks.best_price(), Some(Price(99_00)));

        asks.remove_level(Price(99_00));
        assert_eq!(asks.best_price(), Some(Price(100_00)));
    }

    // === Order operations ===

    #[test]
    fn insert_multiple_orders_same_price() {
        let mut bids = PriceLevels::new(Side::Buy);

        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(100_00), OrderId(2), 200);
        bids.insert_order(Price(100_00), OrderId(3), 150);

        assert_eq!(bids.level_count(), 1);
        let level = bids.best_level().unwrap();
        assert_eq!(level.order_count(), 3);
        assert_eq!(level.total_quantity(), 450);
    }

    #[test]
    fn remove_order_removes_empty_level() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(99_00), OrderId(2), 200);

        assert_eq!(bids.level_count(), 2);

        // Remove the only order at 100
        assert!(bids.remove_order(Price(100_00), OrderId(1), 100));
        assert_eq!(bids.level_count(), 1);
        assert_eq!(bids.best_price(), Some(Price(99_00)));
    }

    #[test]
    fn remove_order_keeps_nonempty_level() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(100_00), OrderId(2), 200);

        assert!(bids.remove_order(Price(100_00), OrderId(1), 100));
        assert_eq!(bids.level_count(), 1);

        let level = bids.best_level().unwrap();
        assert_eq!(level.order_count(), 1);
        assert_eq!(level.total_quantity(), 200);
    }

    #[test]
    fn remove_nonexistent_order() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);

        assert!(!bids.remove_order(Price(100_00), OrderId(999), 100));
        assert!(!bids.remove_order(Price(999_00), OrderId(1), 100));
    }

    // === Iteration ===

    #[test]
    fn iter_bids_best_to_worst() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(99_00), OrderId(1), 100);
        bids.insert_order(Price(101_00), OrderId(2), 100);
        bids.insert_order(Price(100_00), OrderId(3), 100);

        let prices: Vec<_> = bids.iter_best_to_worst().map(|(p, _)| *p).collect();
        assert_eq!(prices, vec![Price(101_00), Price(100_00), Price(99_00)]);
    }

    #[test]
    fn iter_asks_best_to_worst() {
        let mut asks = PriceLevels::new(Side::Sell);
        asks.insert_order(Price(99_00), OrderId(1), 100);
        asks.insert_order(Price(101_00), OrderId(2), 100);
        asks.insert_order(Price(100_00), OrderId(3), 100);

        let prices: Vec<_> = asks.iter_best_to_worst().map(|(p, _)| *p).collect();
        assert_eq!(prices, vec![Price(99_00), Price(100_00), Price(101_00)]);
    }

    // === Quantity queries ===

    #[test]
    fn total_quantity() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(100_00), OrderId(2), 200);
        bids.insert_order(Price(99_00), OrderId(3), 150);

        assert_eq!(bids.total_quantity(), 450);
    }

    #[test]
    fn quantity_at_or_better_bids() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);
        bids.insert_order(Price(99_00), OrderId(2), 200);
        bids.insert_order(Price(98_00), OrderId(3), 150);

        // Bids at or above $99 (prices >= 99_00)
        assert_eq!(bids.quantity_at_or_better(Price(99_00)), 300); // 100 + 200

        // Bids at or above $100
        assert_eq!(bids.quantity_at_or_better(Price(100_00)), 100);

        // Bids at or above $98
        assert_eq!(bids.quantity_at_or_better(Price(98_00)), 450);
    }

    #[test]
    fn quantity_at_or_better_asks() {
        let mut asks = PriceLevels::new(Side::Sell);
        asks.insert_order(Price(100_00), OrderId(1), 100);
        asks.insert_order(Price(101_00), OrderId(2), 200);
        asks.insert_order(Price(102_00), OrderId(3), 150);

        // Asks at or below $101 (prices <= 101_00)
        assert_eq!(asks.quantity_at_or_better(Price(101_00)), 300); // 100 + 200

        // Asks at or below $100
        assert_eq!(asks.quantity_at_or_better(Price(100_00)), 100);

        // Asks at or below $102
        assert_eq!(asks.quantity_at_or_better(Price(102_00)), 450);
    }

    // === Best level mutation ===

    #[test]
    fn best_level_mut_allows_modification() {
        let mut bids = PriceLevels::new(Side::Buy);
        bids.insert_order(Price(100_00), OrderId(1), 100);

        if let Some(level) = bids.best_level_mut() {
            level.decrease_quantity(30);
        }

        assert_eq!(bids.best_level().unwrap().total_quantity(), 70);
    }

    #[test]
    fn pop_best_level() {
        let mut asks = PriceLevels::new(Side::Sell);
        asks.insert_order(Price(100_00), OrderId(1), 100);
        asks.insert_order(Price(101_00), OrderId(2), 200);

        let popped = asks.pop_best_level().unwrap();
        assert_eq!(popped.price(), Price(100_00));
        assert_eq!(asks.best_price(), Some(Price(101_00)));
    }
}
