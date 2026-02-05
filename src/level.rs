//! Level: A FIFO queue of orders at a single price point.
//!
//! The Level stores only `OrderId`s, not full `Order` objects.
//! Orders themselves live in a central `HashMap` for O(1) lookup.

use std::collections::VecDeque;

use crate::{OrderId, Price, Quantity};

/// A queue of orders at a single price level.
///
/// Orders are processed FIFO (first-in-first-out) for time priority.
/// The level tracks total quantity for efficient depth queries.
#[derive(Clone, Debug)]
pub struct Level {
    /// The price for all orders in this level
    price: Price,
    /// Order IDs in FIFO order
    orders: VecDeque<OrderId>,
    /// Sum of remaining quantities (cached for O(1) access)
    total_quantity: Quantity,
}

impl Level {
    /// Create a new empty level at the given price.
    pub fn new(price: Price) -> Self {
        Self {
            price,
            orders: VecDeque::new(),
            total_quantity: 0,
        }
    }

    /// Returns the price of this level.
    #[inline]
    pub fn price(&self) -> Price {
        self.price
    }

    /// Returns true if there are no orders at this level.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    /// Returns the number of orders at this level.
    #[inline]
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }

    /// Returns the total quantity across all orders at this level.
    #[inline]
    pub fn total_quantity(&self) -> Quantity {
        self.total_quantity
    }

    /// Returns the OrderId at the front of the queue (next to fill).
    #[inline]
    pub fn front(&self) -> Option<OrderId> {
        self.orders.front().copied()
    }

    /// Add an order to the back of the queue.
    ///
    /// The quantity is added to the level's total.
    pub fn push_back(&mut self, order_id: OrderId, quantity: Quantity) {
        self.orders.push_back(order_id);
        self.total_quantity += quantity;
    }

    /// Remove and return the order at the front of the queue.
    ///
    /// The provided quantity is subtracted from the level's total.
    /// This should be the order's remaining quantity at time of removal.
    ///
    /// Returns `None` if the level is empty.
    pub fn pop_front(&mut self, quantity: Quantity) -> Option<OrderId> {
        let order_id = self.orders.pop_front()?;
        self.total_quantity = self.total_quantity.saturating_sub(quantity);
        Some(order_id)
    }

    /// Remove a specific order from anywhere in the queue (for cancellation).
    ///
    /// Returns `true` if the order was found and removed, `false` otherwise.
    /// The provided quantity is subtracted from the level's total.
    ///
    /// Note: This is O(n) where n is the number of orders at this price level.
    /// For O(1) cancel, consider a tombstone approach or intrusive linked list.
    pub fn remove(&mut self, order_id: OrderId, quantity: Quantity) -> bool {
        if let Some(pos) = self.orders.iter().position(|&id| id == order_id) {
            self.orders.remove(pos);
            self.total_quantity = self.total_quantity.saturating_sub(quantity);
            true
        } else {
            false
        }
    }

    /// Decrease the total quantity (e.g., after a partial fill).
    ///
    /// Use this when an order is partially filled but remains in the queue.
    pub fn decrease_quantity(&mut self, amount: Quantity) {
        self.total_quantity = self.total_quantity.saturating_sub(amount);
    }

    /// Returns an iterator over the order IDs in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = OrderId> + '_ {
        self.orders.iter().copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_level_is_empty() {
        let level = Level::new(Price(100_00));

        assert!(level.is_empty());
        assert_eq!(level.order_count(), 0);
        assert_eq!(level.total_quantity(), 0);
        assert_eq!(level.front(), None);
        assert_eq!(level.price(), Price(100_00));
    }

    #[test]
    fn push_back_adds_orders() {
        let mut level = Level::new(Price(100_00));

        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        assert!(!level.is_empty());
        assert_eq!(level.order_count(), 3);
        assert_eq!(level.total_quantity(), 450);
        assert_eq!(level.front(), Some(OrderId(1)));
    }

    #[test]
    fn pop_front_fifo_order() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        // Pop in FIFO order
        assert_eq!(level.pop_front(100), Some(OrderId(1)));
        assert_eq!(level.total_quantity(), 350);
        assert_eq!(level.front(), Some(OrderId(2)));

        assert_eq!(level.pop_front(200), Some(OrderId(2)));
        assert_eq!(level.total_quantity(), 150);
        assert_eq!(level.front(), Some(OrderId(3)));

        assert_eq!(level.pop_front(150), Some(OrderId(3)));
        assert_eq!(level.total_quantity(), 0);
        assert!(level.is_empty());

        // Empty level returns None
        assert_eq!(level.pop_front(0), None);
    }

    #[test]
    fn remove_from_middle() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        // Remove middle order
        assert!(level.remove(OrderId(2), 200));
        assert_eq!(level.order_count(), 2);
        assert_eq!(level.total_quantity(), 250);

        // FIFO order preserved for remaining
        assert_eq!(level.pop_front(100), Some(OrderId(1)));
        assert_eq!(level.pop_front(150), Some(OrderId(3)));
    }

    #[test]
    fn remove_nonexistent_returns_false() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);

        assert!(!level.remove(OrderId(999), 50));
        assert_eq!(level.order_count(), 1);
        assert_eq!(level.total_quantity(), 100);
    }

    #[test]
    fn remove_from_front() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);

        assert!(level.remove(OrderId(1), 100));
        assert_eq!(level.front(), Some(OrderId(2)));
    }

    #[test]
    fn remove_from_back() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);

        assert!(level.remove(OrderId(2), 200));
        assert_eq!(level.front(), Some(OrderId(1)));
        assert_eq!(level.order_count(), 1);
    }

    #[test]
    fn decrease_quantity_for_partial_fill() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);

        // Partial fill of order 1: filled 30, remaining 70
        level.decrease_quantity(30);

        assert_eq!(level.total_quantity(), 270);
        assert_eq!(level.order_count(), 2); // Order still in queue
    }

    #[test]
    fn iter_returns_fifo_order() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        let ids: Vec<_> = level.iter().collect();
        assert_eq!(ids, vec![OrderId(1), OrderId(2), OrderId(3)]);
    }

    #[test]
    fn quantity_saturates_on_underflow() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);

        // Try to subtract more than available (shouldn't happen in practice)
        level.decrease_quantity(200);
        assert_eq!(level.total_quantity(), 0);

        // Pop with excessive quantity
        level.push_back(OrderId(2), 50);
        level.pop_front(100);
        assert_eq!(level.total_quantity(), 0);
    }
}
