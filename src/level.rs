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
    pub(crate) orders: VecDeque<OrderId>,
    /// Sum of remaining quantities (cached for O(1) access)
    total_quantity: Quantity,
    /// Number of tombstones (cancelled orders still in the queue)
    tombstone_count: usize,
}

impl Level {
    /// Create a new empty level at the given price.
    pub fn new(price: Price) -> Self {
        Self {
            price,
            orders: VecDeque::new(),
            total_quantity: 0,
            tombstone_count: 0,
        }
    }

    /// Returns the price of this level.
    #[inline]
    pub fn price(&self) -> Price {
        self.price
    }

    /// Returns true if there are no active orders at this level.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.total_quantity == 0
    }

    /// Returns the number of orders at this level (including tombstones).
    #[inline]
    pub fn order_count(&self) -> usize {
        self.orders.len() - self.tombstone_count
    }

    /// Returns the total quantity across all orders at this level.
    #[inline]
    pub fn total_quantity(&self) -> Quantity {
        self.total_quantity
    }

    /// Returns the number of tombstones in the queue.
    #[inline]
    pub fn tombstone_count(&self) -> usize {
        self.tombstone_count
    }

    /// Returns the OrderId at the front of the queue (next to fill).
    /// Skips tombstones.
    pub fn front(&mut self) -> Option<OrderId> {
        while let Some(&id) = self.orders.front() {
            if id.0 == 0 {
                // It's a tombstone
                self.orders.pop_front();
                self.tombstone_count -= 1;
            } else {
                return Some(id);
            }
        }
        None
    }

    /// Add an order to the back of the queue.
    ///
    /// The quantity is added to the level's total (saturating on overflow).
    pub fn push_back(&mut self, order_id: OrderId, quantity: Quantity) {
        assert!(order_id.0 != 0, "OrderId(0) reserved for tombstones");
        self.orders.push_back(order_id);
        self.total_quantity = self.total_quantity.saturating_add(quantity);
    }

    /// Remove and return the order at the front of the queue.
    ///
    /// The provided quantity is subtracted from the level's total.
    /// This should be the order's remaining quantity at time of removal.
    ///
    /// Returns `None` if the level is empty.
    pub fn pop_front(&mut self, quantity: Quantity) -> Option<OrderId> {
        while let Some(id) = self.orders.pop_front() {
            if id.0 == 0 {
                self.tombstone_count -= 1;
                continue;
            }
            self.total_quantity = self.total_quantity.saturating_sub(quantity);
            return Some(id);
        }
        None
    }

    /// Mark an order as a tombstone.
    ///
    /// This is O(1) if you know the position, but here we still don't.
    /// Wait, the plan said: "HashMap<OrderId, (Price, Side, usize)> stores position index".
    /// That means I need to store the index in OrderBook.
    pub fn mark_tombstone(&mut self, index: usize, quantity: Quantity) {
        if let Some(id_ref) = self.orders.get_mut(index) {
            if id_ref.0 != 0 {
                id_ref.0 = 0; // Set to tombstone ID
                self.total_quantity = self.total_quantity.saturating_sub(quantity);
                self.tombstone_count += 1;
            }
        }
    }

    /// Remove a specific order from anywhere in the queue (for cancellation).
    ///
    /// Returns `true` if the order was found and removed, `false` otherwise.
    /// The provided quantity is subtracted from the level's total.
    ///
    /// Note: This is O(n) where n is the number of orders at this price level.
    /// For O(1) cancel, we now use `mark_tombstone` called from OrderBook.
    pub fn remove(&mut self, order_id: OrderId, quantity: Quantity) -> bool {
        if let Some(pos) = self.orders.iter().position(|&id| id == order_id) {
            self.orders.remove(pos);
            self.total_quantity = self.total_quantity.saturating_sub(quantity);
            true
        } else {
            false
        }
    }

    /// Remove all tombstones from the queue.
    pub fn compact(&mut self) {
        if self.tombstone_count == 0 {
            return;
        }
        self.orders.retain(|id| id.0 != 0);
        self.tombstone_count = 0;
    }

    /// Decrease the total quantity (e.g., after a partial fill).
    ///
    /// Use this when an order is partially filled but remains in the queue.
    pub fn decrease_quantity(&mut self, amount: Quantity) {
        self.total_quantity = self.total_quantity.saturating_sub(amount);
    }

    /// Returns an iterator over the active order IDs in FIFO order.
    pub fn iter(&self) -> impl Iterator<Item = OrderId> + '_ {
        self.orders.iter().copied().filter(|id| id.0 != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_level_is_empty() {
        let mut level = Level::new(Price(100_00));

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
    fn tombstone_marking() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        // Mark middle as tombstone
        level.mark_tombstone(1, 200);

        assert_eq!(level.order_count(), 2);
        assert_eq!(level.total_quantity(), 250);
        assert_eq!(level.tombstone_count(), 1);

        // Iterator should skip it
        let ids: Vec<_> = level.iter().collect();
        assert_eq!(ids, vec![OrderId(1), OrderId(3)]);
    }

    #[test]
    fn tombstone_compaction() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);
        level.push_back(OrderId(3), 150);

        level.mark_tombstone(0, 100);
        level.mark_tombstone(2, 150);

        assert_eq!(level.orders.len(), 3);
        assert_eq!(level.tombstone_count(), 2);

        level.compact();

        assert_eq!(level.orders.len(), 1);
        assert_eq!(level.tombstone_count(), 0);
        assert_eq!(level.orders[0], OrderId(2));
    }

    #[test]
    fn front_skips_tombstones() {
        let mut level = Level::new(Price(100_00));
        level.push_back(OrderId(1), 100);
        level.push_back(OrderId(2), 200);

        level.mark_tombstone(0, 100);

        // front() should return OrderId(2) and remove the tombstone from the front
        assert_eq!(level.front(), Some(OrderId(2)));
        assert_eq!(level.orders.len(), 1);
        assert_eq!(level.tombstone_count(), 0);
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
