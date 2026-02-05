//! OrderBook: The complete order book with both sides and order storage.
//!
//! This is the core data structure that combines:
//! - Bids (buy orders) sorted high → low
//! - Asks (sell orders) sorted low → high
//! - Central order storage for O(1) lookup by OrderId

use rustc_hash::FxHashMap;

use crate::{Order, OrderId, Price, PriceLevels, Quantity, Side, TimeInForce, Timestamp, TradeId};

// Re-import for tests only
#[cfg(test)]
use crate::OrderStatus;

/// The complete order book.
///
/// Maintains both sides of the book plus a central index of all orders
/// (active and historical) for O(1) lookup.
#[derive(Clone, Debug)]
pub struct OrderBook {
    /// Buy orders, sorted by price descending (best = highest)
    bids: PriceLevels,
    /// Sell orders, sorted by price ascending (best = lowest)
    asks: PriceLevels,
    /// All orders indexed by ID (includes filled/cancelled for history)
    pub(crate) orders: FxHashMap<OrderId, Order>,
    /// Next order ID to assign
    next_order_id: u64,
    /// Next trade ID to assign
    next_trade_id: u64,
    /// Next timestamp to assign (monotonic counter)
    next_timestamp: u64,
}

impl OrderBook {
    /// Create a new empty order book.
    pub fn new() -> Self {
        Self {
            bids: PriceLevels::new(Side::Buy),
            asks: PriceLevels::new(Side::Sell),
            orders: FxHashMap::default(),
            next_order_id: 1,
            next_trade_id: 1,
            next_timestamp: 1,
        }
    }

    // === ID and timestamp generation ===

    /// Generate the next order ID (monotonically increasing).
    pub fn next_order_id(&mut self) -> OrderId {
        let id = OrderId(self.next_order_id);
        self.next_order_id += 1;
        id
    }

    /// Generate the next trade ID (monotonically increasing).
    pub fn next_trade_id(&mut self) -> TradeId {
        let id = TradeId(self.next_trade_id);
        self.next_trade_id += 1;
        id
    }

    /// Generate the next timestamp (monotonically increasing).
    pub fn next_timestamp(&mut self) -> Timestamp {
        let ts = self.next_timestamp;
        self.next_timestamp += 1;
        ts
    }

    /// Peek at what the next order ID would be (without consuming it).
    pub fn peek_next_order_id(&self) -> OrderId {
        OrderId(self.next_order_id)
    }

    // === Order access ===

    /// Get an order by ID (includes historical filled/cancelled orders).
    pub fn get_order(&self, order_id: OrderId) -> Option<&Order> {
        self.orders.get(&order_id)
    }

    /// Get a mutable reference to an order by ID.
    pub fn get_order_mut(&mut self, order_id: OrderId) -> Option<&mut Order> {
        self.orders.get_mut(&order_id)
    }

    /// Check if an order exists.
    pub fn contains_order(&self, order_id: OrderId) -> bool {
        self.orders.contains_key(&order_id)
    }

    /// Returns the total number of orders (including historical).
    pub fn order_count(&self) -> usize {
        self.orders.len()
    }

    /// Returns the number of active orders (on the book).
    pub fn active_order_count(&self) -> usize {
        self.orders.values().filter(|o| o.is_active()).count()
    }

    // === Book access ===

    /// Get the bids side (buy orders).
    pub fn bids(&self) -> &PriceLevels {
        &self.bids
    }

    /// Get the asks side (sell orders).
    pub fn asks(&self) -> &PriceLevels {
        &self.asks
    }

    /// Get mutable access to bids.
    pub fn bids_mut(&mut self) -> &mut PriceLevels {
        &mut self.bids
    }

    /// Get mutable access to asks.
    pub fn asks_mut(&mut self) -> &mut PriceLevels {
        &mut self.asks
    }

    /// Get the appropriate side for an order.
    pub fn side(&self, side: Side) -> &PriceLevels {
        match side {
            Side::Buy => &self.bids,
            Side::Sell => &self.asks,
        }
    }

    /// Get mutable access to the appropriate side.
    pub fn side_mut(&mut self, side: Side) -> &mut PriceLevels {
        match side {
            Side::Buy => &mut self.bids,
            Side::Sell => &mut self.asks,
        }
    }

    /// Get the opposite side (for matching).
    pub fn opposite_side(&self, side: Side) -> &PriceLevels {
        self.side(side.opposite())
    }

    /// Get mutable access to the opposite side.
    pub fn opposite_side_mut(&mut self, side: Side) -> &mut PriceLevels {
        self.side_mut(side.opposite())
    }

    // === Best prices ===

    /// Get the best bid price (highest buy price).
    pub fn best_bid(&self) -> Option<Price> {
        self.bids.best_price()
    }

    /// Get the best ask price (lowest sell price).
    pub fn best_ask(&self) -> Option<Price> {
        self.asks.best_price()
    }

    /// Get both best bid and best ask.
    pub fn best_bid_ask(&self) -> (Option<Price>, Option<Price>) {
        (self.best_bid(), self.best_ask())
    }

    /// Get the spread (best ask - best bid), if both exist.
    pub fn spread(&self) -> Option<i64> {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => Some(ask.0 - bid.0),
            _ => None,
        }
    }

    /// Check if the book is crossed (best bid >= best ask).
    /// This should never happen after matching is complete.
    pub fn is_crossed(&self) -> bool {
        match (self.best_bid(), self.best_ask()) {
            (Some(bid), Some(ask)) => bid >= ask,
            _ => false,
        }
    }

    // === Order management ===

    /// Add a new order to the book.
    ///
    /// The order must have a unique ID (typically from `next_order_id()`).
    /// The order is stored in the central index and added to the appropriate
    /// price level based on its side and price.
    ///
    /// # Panics
    ///
    /// Panics if an order with the same ID already exists.
    pub fn add_order(&mut self, order: Order) {
        assert!(
            !self.orders.contains_key(&order.id),
            "order {} already exists",
            order.id
        );

        let side = order.side;
        let price = order.price;
        let quantity = order.remaining_quantity;
        let order_id = order.id;

        // Store in central index
        self.orders.insert(order_id, order);

        // Add to appropriate price level
        self.side_mut(side).insert_order(price, order_id, quantity);
    }

    /// Remove an order from the book (for cancellation).
    ///
    /// Updates the order's status to Cancelled and removes it from
    /// the price level queue. The order remains in the central index
    /// for historical queries.
    ///
    /// Returns the cancelled quantity, or None if order not found or not active.
    pub fn cancel_order(&mut self, order_id: OrderId) -> Option<Quantity> {
        let order = self.orders.get_mut(&order_id)?;

        if !order.is_active() {
            return None;
        }

        let side = order.side;
        let price = order.price;
        let remaining = order.remaining_quantity;

        // Cancel the order (updates status)
        order.cancel();

        // Remove from price level
        self.side_mut(side).remove_order(price, order_id, remaining);

        Some(remaining)
    }

    /// Create a new order with auto-generated ID and timestamp.
    ///
    /// This is a convenience method that:
    /// 1. Generates the next order ID
    /// 2. Generates the next timestamp
    /// 3. Creates the Order struct
    ///
    /// The order is NOT added to the book — use `add_order()` for that.
    pub fn create_order(
        &mut self,
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
    ) -> Order {
        let id = self.next_order_id();
        let timestamp = self.next_timestamp();
        Order::new(id, side, price, quantity, timestamp, time_in_force)
    }

    /// Remove filled and cancelled orders from history to free memory.
    ///
    /// Active orders (on the book) are preserved. Returns the number of
    /// orders removed.
    ///
    /// Use this periodically for long-running instances to prevent
    /// unbounded memory growth.
    pub fn clear_history(&mut self) -> usize {
        let before = self.orders.len();
        self.orders.retain(|_, order| order.is_active());
        before - self.orders.len()
    }
}

impl Default for OrderBook {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_book_is_empty() {
        let book = OrderBook::new();

        assert_eq!(book.order_count(), 0);
        assert_eq!(book.active_order_count(), 0);
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.spread(), None);
        assert!(!book.is_crossed());
    }

    #[test]
    fn id_generation_is_monotonic() {
        let mut book = OrderBook::new();

        assert_eq!(book.next_order_id(), OrderId(1));
        assert_eq!(book.next_order_id(), OrderId(2));
        assert_eq!(book.next_order_id(), OrderId(3));

        assert_eq!(book.next_trade_id(), TradeId(1));
        assert_eq!(book.next_trade_id(), TradeId(2));

        assert_eq!(book.next_timestamp(), 1);
        assert_eq!(book.next_timestamp(), 2);
    }

    #[test]
    fn peek_order_id_does_not_consume() {
        let mut book = OrderBook::new();

        assert_eq!(book.peek_next_order_id(), OrderId(1));
        assert_eq!(book.peek_next_order_id(), OrderId(1));
        assert_eq!(book.next_order_id(), OrderId(1));
        assert_eq!(book.peek_next_order_id(), OrderId(2));
    }

    #[test]
    fn add_and_get_order() {
        let mut book = OrderBook::new();

        let order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let order_id = order.id;
        book.add_order(order);

        assert!(book.contains_order(order_id));
        assert_eq!(book.order_count(), 1);
        assert_eq!(book.active_order_count(), 1);

        let retrieved = book.get_order(order_id).unwrap();
        assert_eq!(retrieved.id, order_id);
        assert_eq!(retrieved.price, Price(100_00));
        assert_eq!(retrieved.remaining_quantity, 100);
    }

    #[test]
    fn add_order_updates_best_prices() {
        let mut book = OrderBook::new();

        // Add a bid
        let bid = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        book.add_order(bid);
        assert_eq!(book.best_bid(), Some(Price(100_00)));
        assert_eq!(book.best_ask(), None);

        // Add an ask
        let ask = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        book.add_order(ask);
        assert_eq!(book.best_bid(), Some(Price(100_00)));
        assert_eq!(book.best_ask(), Some(Price(101_00)));
    }

    #[test]
    fn spread_calculation() {
        let mut book = OrderBook::new();

        // No spread without both sides
        assert_eq!(book.spread(), None);

        let bid = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        book.add_order(bid);
        assert_eq!(book.spread(), None);

        let ask = book.create_order(Side::Sell, Price(101_50), 100, TimeInForce::GTC);
        book.add_order(ask);
        assert_eq!(book.spread(), Some(150)); // 101.50 - 100.00 = 1.50 = 150 cents
    }

    #[test]
    fn cancel_order() {
        let mut book = OrderBook::new();

        let order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let order_id = order.id;
        book.add_order(order);

        assert_eq!(book.active_order_count(), 1);
        assert_eq!(book.best_bid(), Some(Price(100_00)));

        // Cancel it
        let cancelled = book.cancel_order(order_id);
        assert_eq!(cancelled, Some(100));

        // Order still exists but is not active
        assert_eq!(book.order_count(), 1);
        assert_eq!(book.active_order_count(), 0);
        assert_eq!(
            book.get_order(order_id).unwrap().status,
            OrderStatus::Cancelled
        );

        // Best bid is now gone
        assert_eq!(book.best_bid(), None);
    }

    #[test]
    fn cancel_nonexistent_order() {
        let mut book = OrderBook::new();
        assert_eq!(book.cancel_order(OrderId(999)), None);
    }

    #[test]
    fn cancel_already_cancelled() {
        let mut book = OrderBook::new();

        let order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let order_id = order.id;
        book.add_order(order);

        book.cancel_order(order_id);
        assert_eq!(book.cancel_order(order_id), None); // Already cancelled
    }

    #[test]
    fn multiple_orders_same_price() {
        let mut book = OrderBook::new();

        let o1 = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let o2 = book.create_order(Side::Buy, Price(100_00), 200, TimeInForce::GTC);
        let o3 = book.create_order(Side::Buy, Price(100_00), 150, TimeInForce::GTC);

        book.add_order(o1);
        book.add_order(o2);
        book.add_order(o3);

        assert_eq!(book.active_order_count(), 3);
        assert_eq!(book.bids().level_count(), 1);
        assert_eq!(book.bids().total_quantity(), 450);
    }

    #[test]
    fn multiple_price_levels() {
        let mut book = OrderBook::new();

        let o1 = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let o2 = book.create_order(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
        let o3 = book.create_order(Side::Sell, Price(101_00), 150, TimeInForce::GTC);
        let o4 = book.create_order(Side::Sell, Price(102_00), 175, TimeInForce::GTC);

        book.add_order(o1);
        book.add_order(o2);
        book.add_order(o3);
        book.add_order(o4);

        assert_eq!(book.bids().level_count(), 2);
        assert_eq!(book.asks().level_count(), 2);
        assert_eq!(book.best_bid(), Some(Price(100_00)));
        assert_eq!(book.best_ask(), Some(Price(101_00)));
    }

    #[test]
    fn is_crossed() {
        let mut book = OrderBook::new();

        // Not crossed when empty
        assert!(!book.is_crossed());

        // Not crossed with normal spread
        let bid = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let ask = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        book.add_order(bid);
        book.add_order(ask);
        assert!(!book.is_crossed());

        // Would be crossed if we add a higher bid (in practice, matching prevents this)
        let high_bid = book.create_order(Side::Buy, Price(102_00), 100, TimeInForce::GTC);
        book.add_order(high_bid);
        assert!(book.is_crossed());
    }

    #[test]
    fn opposite_side() {
        let mut book = OrderBook::new();

        let bid = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let ask = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        book.add_order(bid);
        book.add_order(ask);

        // Opposite of buy is sell (asks)
        assert_eq!(
            book.opposite_side(Side::Buy).best_price(),
            Some(Price(101_00))
        );
        // Opposite of sell is buy (bids)
        assert_eq!(
            book.opposite_side(Side::Sell).best_price(),
            Some(Price(100_00))
        );
    }

    #[test]
    #[should_panic(expected = "already exists")]
    fn add_duplicate_order_panics() {
        let mut book = OrderBook::new();

        let order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let order_clone = order.clone();

        book.add_order(order);
        book.add_order(order_clone); // Panic: duplicate ID
    }

    #[test]
    fn get_order_mut() {
        let mut book = OrderBook::new();

        let order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let order_id = order.id;
        book.add_order(order);

        // Modify via mutable reference
        {
            let order = book.get_order_mut(order_id).unwrap();
            order.fill(30);
        }

        // Verify change persisted
        let order = book.get_order(order_id).unwrap();
        assert_eq!(order.remaining_quantity, 70);
        assert_eq!(order.filled_quantity, 30);
    }

    #[test]
    fn clear_history_removes_inactive_orders() {
        let mut book = OrderBook::new();

        // Add some orders
        let o1 = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let o2 = book.create_order(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
        let o3 = book.create_order(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        let o1_id = o1.id;
        let o2_id = o2.id;
        let o3_id = o3.id;

        book.add_order(o1);
        book.add_order(o2);
        book.add_order(o3);

        assert_eq!(book.order_count(), 3);

        // Cancel one order (becomes inactive but stays in history)
        book.cancel_order(o1_id);
        assert_eq!(book.order_count(), 3);
        assert_eq!(book.active_order_count(), 2);

        // Clear history - should remove the cancelled order
        let removed = book.clear_history();
        assert_eq!(removed, 1);
        assert_eq!(book.order_count(), 2);
        assert_eq!(book.active_order_count(), 2);

        // Cancelled order should no longer be accessible
        assert!(book.get_order(o1_id).is_none());
        // Active orders should still be there
        assert!(book.get_order(o2_id).is_some());
        assert!(book.get_order(o3_id).is_some());
    }
}
