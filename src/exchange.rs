//! Exchange: The high-level API for order submission and management.
//!
//! This is the main entry point for users of the library. It wraps the
//! OrderBook and provides methods for submitting orders with proper
//! time-in-force handling.

use crate::{
    event::Event,
    result::{CancelError, CancelResult, ModifyError, ModifyResult, SubmitResult},
    snapshot::BookSnapshot,
    Order, OrderBook, OrderId, OrderStatus, Price, Quantity, Side, TimeInForce, Trade,
};

/// The exchange: processes orders and maintains the order book.
///
/// This is the main interface for interacting with the limit order book.
/// It handles:
/// - Order submission with time-in-force semantics
/// - Order cancellation and modification
/// - Book snapshots for market data
/// - Trade history
/// - Event logging for replay
#[derive(Clone, Debug)]
pub struct Exchange {
    /// The underlying order book
    pub(crate) book: OrderBook,
    /// Complete trade history
    pub(crate) trades: Vec<Trade>,
    /// Event log for replay
    pub(crate) events: Vec<crate::event::Event>,
}

impl Exchange {
    /// Create a new exchange with an empty order book.
    pub fn new() -> Self {
        Self {
            book: OrderBook::new(),
            trades: Vec::new(),
            events: Vec::new(),
        }
    }

    // === Order Submission ===

    /// Submit a limit order.
    ///
    /// The order is matched against the opposite side of the book.
    /// Remaining quantity is handled according to time-in-force:
    /// - **GTC**: Rests on book until filled or cancelled
    /// - **IOC**: Cancelled (never rests)
    /// - **FOK**: If cannot fill entirely, order is rejected (no trades)
    pub fn submit_limit(
        &mut self,
        side: Side,
        price: Price,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> SubmitResult {
        // Record event
        self.events.push(Event::SubmitLimit {
            side,
            price,
            quantity,
            time_in_force: tif,
        });

        self.submit_limit_internal(side, price, quantity, tif)
    }

    /// Submit a market order.
    ///
    /// Market orders execute immediately at the best available prices.
    /// Any unfilled quantity is cancelled (IOC semantics).
    ///
    /// This is equivalent to a limit order at the worst possible price
    /// with IOC time-in-force.
    pub fn submit_market(&mut self, side: Side, quantity: Quantity) -> SubmitResult {
        // Record event
        self.events.push(Event::SubmitMarket { side, quantity });

        // Market order = limit at worst price + IOC
        let price = match side {
            Side::Buy => Price::MAX,
            Side::Sell => Price::MIN,
        };
        self.submit_limit_internal(side, price, quantity, TimeInForce::IOC)
    }

    /// Internal: submit limit order without recording event.
    pub(crate) fn submit_limit_internal(
        &mut self,
        side: Side,
        price: Price,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> SubmitResult {
        // FOK: Check feasibility before doing anything
        if tif == TimeInForce::FOK {
            if !self.book.can_fully_fill(side, price, quantity) {
                // Reject the order - create it just for the ID
                let order = self.book.create_order(side, price, quantity, tif);
                return SubmitResult {
                    order_id: order.id,
                    status: OrderStatus::Cancelled,
                    trades: Vec::new(),
                    filled_quantity: 0,
                    resting_quantity: 0,
                    cancelled_quantity: quantity,
                };
            }
        }

        // Create the order
        let mut order = self.book.create_order(side, price, quantity, tif);
        let order_id = order.id;

        // Match against the book
        let match_result = self.book.match_order(&mut order);

        // Record trades
        self.trades.extend(match_result.trades.iter().cloned());

        let filled = order.filled_quantity;
        let remaining = order.remaining_quantity;

        // Handle remaining quantity based on TIF
        let (status, resting, cancelled) = if remaining == 0 {
            // Fully filled - store for history
            order.status = OrderStatus::Filled;
            self.book.orders.insert(order_id, order);
            (OrderStatus::Filled, 0, 0)
        } else {
            match tif {
                TimeInForce::GTC => {
                    // Rest on book
                    if filled > 0 {
                        order.status = OrderStatus::PartiallyFilled;
                    }
                    self.book.add_order(order);
                    (
                        if filled > 0 {
                            OrderStatus::PartiallyFilled
                        } else {
                            OrderStatus::New
                        },
                        remaining,
                        0,
                    )
                }
                TimeInForce::IOC | TimeInForce::FOK => {
                    // Cancel remainder (FOK shouldn't reach here with remainder)
                    order.status = if filled > 0 {
                        OrderStatus::PartiallyFilled
                    } else {
                        OrderStatus::Cancelled
                    };
                    // Store the order for history but don't add to book
                    self.book.orders.insert(order_id, order);
                    (
                        if filled > 0 {
                            OrderStatus::PartiallyFilled
                        } else {
                            OrderStatus::Cancelled
                        },
                        0,
                        remaining,
                    )
                }
            }
        };

        SubmitResult {
            order_id,
            status,
            trades: match_result.trades,
            filled_quantity: filled,
            resting_quantity: resting,
            cancelled_quantity: cancelled,
        }
    }

    // === Order Management ===

    /// Cancel an order.
    ///
    /// Returns the cancelled quantity if successful.
    pub fn cancel(&mut self, order_id: OrderId) -> CancelResult {
        // Record event
        self.events.push(Event::Cancel { order_id });

        self.cancel_internal(order_id)
    }

    /// Internal: cancel without recording event.
    pub(crate) fn cancel_internal(&mut self, order_id: OrderId) -> CancelResult {
        // Check if order exists
        let order = match self.book.get_order(order_id) {
            Some(o) => o,
            None => return CancelResult::failure(CancelError::OrderNotFound),
        };

        // Check if order is active
        if !order.is_active() {
            return CancelResult::failure(CancelError::OrderNotActive);
        }

        // Cancel it
        match self.book.cancel_order(order_id) {
            Some(qty) => CancelResult::success(qty),
            None => CancelResult::failure(CancelError::OrderNotActive),
        }
    }

    /// Modify an order (cancel and replace).
    ///
    /// The old order is cancelled and a new order is submitted with
    /// the new price and quantity. The new order gets a new ID and
    /// **loses time priority**.
    ///
    /// The new order inherits the original order's time-in-force.
    pub fn modify(
        &mut self,
        order_id: OrderId,
        new_price: Price,
        new_quantity: Quantity,
    ) -> ModifyResult {
        // Record event (even if it fails)
        self.events.push(Event::Modify {
            order_id,
            new_price,
            new_quantity,
        });

        self.modify_internal(order_id, new_price, new_quantity)
    }

    /// Internal: modify without recording event.
    pub(crate) fn modify_internal(
        &mut self,
        order_id: OrderId,
        new_price: Price,
        new_quantity: Quantity,
    ) -> ModifyResult {
        // Validate quantity
        if new_quantity == 0 {
            return ModifyResult::failure(order_id, ModifyError::InvalidQuantity);
        }

        // Get the old order's details
        let (side, tif) = match self.book.get_order(order_id) {
            Some(o) if o.is_active() => (o.side, o.time_in_force),
            Some(_) => return ModifyResult::failure(order_id, ModifyError::OrderNotActive),
            None => return ModifyResult::failure(order_id, ModifyError::OrderNotFound),
        };

        // Cancel the old order
        let cancelled = match self.book.cancel_order(order_id) {
            Some(qty) => qty,
            None => return ModifyResult::failure(order_id, ModifyError::OrderNotActive),
        };

        // Submit the new order
        let result = self.submit_limit_internal(side, new_price, new_quantity, tif);

        ModifyResult::success(order_id, result.order_id, cancelled, result.trades)
    }

    // === Queries ===

    /// Get an order by ID.
    pub fn get_order(&self, order_id: OrderId) -> Option<&Order> {
        self.book.get_order(order_id)
    }

    /// Get the best bid and ask prices.
    pub fn best_bid_ask(&self) -> (Option<Price>, Option<Price>) {
        self.book.best_bid_ask()
    }

    /// Get the best bid price.
    pub fn best_bid(&self) -> Option<Price> {
        self.book.best_bid()
    }

    /// Get the best ask price.
    pub fn best_ask(&self) -> Option<Price> {
        self.book.best_ask()
    }

    /// Get the spread (best ask - best bid).
    pub fn spread(&self) -> Option<i64> {
        self.book.spread()
    }

    /// Get a snapshot of the top N levels on each side.
    pub fn depth(&self, levels: usize) -> BookSnapshot {
        self.book.snapshot(levels)
    }

    /// Get a full snapshot of the order book.
    pub fn full_book(&self) -> BookSnapshot {
        self.book.full_snapshot()
    }

    /// Get all trades that have occurred.
    pub fn trades(&self) -> &[Trade] {
        &self.trades
    }

    /// Get the underlying order book (for advanced queries).
    pub fn book(&self) -> &OrderBook {
        &self.book
    }
}

impl Default for Exchange {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // === Basic submission ===

    #[test]
    fn submit_limit_no_match() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        assert_eq!(result.order_id, OrderId(1));
        assert_eq!(result.status, OrderStatus::New);
        assert!(result.trades.is_empty());
        assert_eq!(result.filled_quantity, 0);
        assert_eq!(result.resting_quantity, 100);
        assert_eq!(result.cancelled_quantity, 0);

        // Order should be on the book
        assert_eq!(exchange.best_bid(), Some(Price(100_00)));
    }

    #[test]
    fn submit_limit_full_fill() {
        let mut exchange = Exchange::new();

        // Place a resting ask
        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        // Place a crossing bid
        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        assert_eq!(result.status, OrderStatus::Filled);
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.filled_quantity, 100);
        assert_eq!(result.resting_quantity, 0);
        assert_eq!(result.cancelled_quantity, 0);

        // Book should be empty
        assert_eq!(exchange.best_bid(), None);
        assert_eq!(exchange.best_ask(), None);
    }

    #[test]
    fn submit_limit_partial_fill_gtc() {
        let mut exchange = Exchange::new();

        // Place a small ask
        exchange.submit_limit(Side::Sell, Price(100_00), 30, TimeInForce::GTC);

        // Place a larger bid
        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        assert_eq!(result.status, OrderStatus::PartiallyFilled);
        assert_eq!(result.filled_quantity, 30);
        assert_eq!(result.resting_quantity, 70);
        assert_eq!(result.cancelled_quantity, 0);

        // Remainder should be on book
        assert_eq!(exchange.best_bid(), Some(Price(100_00)));
    }

    // === IOC ===

    #[test]
    fn submit_ioc_full_fill() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);

        assert_eq!(result.status, OrderStatus::Filled);
        assert_eq!(result.filled_quantity, 100);
        assert_eq!(result.resting_quantity, 0);
    }

    #[test]
    fn submit_ioc_partial_fill() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 30, TimeInForce::GTC);

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);

        assert_eq!(result.status, OrderStatus::PartiallyFilled);
        assert_eq!(result.filled_quantity, 30);
        assert_eq!(result.resting_quantity, 0); // IOC never rests
        assert_eq!(result.cancelled_quantity, 70);

        // Nothing on bid side
        assert_eq!(exchange.best_bid(), None);
    }

    #[test]
    fn submit_ioc_no_fill() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);

        assert_eq!(result.status, OrderStatus::Cancelled);
        assert_eq!(result.filled_quantity, 0);
        assert_eq!(result.cancelled_quantity, 100);
        assert_eq!(exchange.best_bid(), None);
    }

    // === FOK ===

    #[test]
    fn submit_fok_full_fill() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);

        assert_eq!(result.status, OrderStatus::Filled);
        assert_eq!(result.filled_quantity, 100);
        assert_eq!(result.trades.len(), 1);
    }

    #[test]
    fn submit_fok_rejected_insufficient_liquidity() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);

        // Try to buy 100 but only 50 available
        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);

        assert_eq!(result.status, OrderStatus::Cancelled);
        assert_eq!(result.filled_quantity, 0);
        assert_eq!(result.cancelled_quantity, 100);
        assert!(result.trades.is_empty()); // No trades!

        // Ask should still be there
        assert_eq!(exchange.best_ask(), Some(Price(100_00)));
    }

    #[test]
    fn submit_fok_rejected_no_liquidity() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);

        assert_eq!(result.status, OrderStatus::Cancelled);
        assert!(result.trades.is_empty());
    }

    // === Market orders ===

    #[test]
    fn submit_market_full_fill() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        let result = exchange.submit_market(Side::Buy, 100);

        assert_eq!(result.status, OrderStatus::Filled);
        assert_eq!(result.filled_quantity, 100);
    }

    #[test]
    fn submit_market_partial_fill() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);

        let result = exchange.submit_market(Side::Buy, 100);

        assert_eq!(result.status, OrderStatus::PartiallyFilled);
        assert_eq!(result.filled_quantity, 50);
        assert_eq!(result.cancelled_quantity, 50);
    }

    #[test]
    fn submit_market_no_liquidity() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_market(Side::Buy, 100);

        assert_eq!(result.status, OrderStatus::Cancelled);
        assert_eq!(result.filled_quantity, 0);
    }

    // === Cancel ===

    #[test]
    fn cancel_order() {
        let mut exchange = Exchange::new();

        let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.cancel(submit.order_id);

        assert!(result.success);
        assert_eq!(result.cancelled_quantity, 100);
        assert_eq!(exchange.best_bid(), None);
    }

    #[test]
    fn cancel_nonexistent() {
        let mut exchange = Exchange::new();

        let result = exchange.cancel(OrderId(999));

        assert!(!result.success);
        assert_eq!(result.error, Some(CancelError::OrderNotFound));
    }

    #[test]
    fn cancel_already_filled() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        let buy = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        // Order is filled, can't cancel
        let result = exchange.cancel(buy.order_id);

        assert!(!result.success);
        assert_eq!(result.error, Some(CancelError::OrderNotActive));
    }

    // === Modify ===

    #[test]
    fn modify_order() {
        let mut exchange = Exchange::new();

        let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.modify(submit.order_id, Price(99_00), 150);

        assert!(result.success);
        assert_eq!(result.old_order_id, submit.order_id);
        assert!(result.new_order_id.is_some());
        assert_ne!(result.new_order_id.unwrap(), submit.order_id);
        assert_eq!(result.cancelled_quantity, 100);

        // New order should be on book at new price
        assert_eq!(exchange.best_bid(), Some(Price(99_00)));
        let new_order = exchange.get_order(result.new_order_id.unwrap()).unwrap();
        assert_eq!(new_order.remaining_quantity, 150);
    }

    #[test]
    fn modify_with_immediate_fill() {
        let mut exchange = Exchange::new();

        // Resting ask
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);

        // Resting bid that doesn't cross
        let submit = exchange.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);

        // Modify to cross
        let result = exchange.modify(submit.order_id, Price(100_00), 100);

        assert!(result.success);
        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].quantity, 50);
    }

    #[test]
    fn modify_nonexistent() {
        let mut exchange = Exchange::new();

        let result = exchange.modify(OrderId(999), Price(100_00), 100);

        assert!(!result.success);
        assert_eq!(result.error, Some(ModifyError::OrderNotFound));
    }

    #[test]
    fn modify_zero_quantity() {
        let mut exchange = Exchange::new();

        let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.modify(submit.order_id, Price(100_00), 0);

        assert!(!result.success);
        assert_eq!(result.error, Some(ModifyError::InvalidQuantity));
    }

    // === Queries ===

    #[test]
    fn trades_are_recorded() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        assert_eq!(exchange.trades().len(), 1);
        assert_eq!(exchange.trades()[0].quantity, 100);
    }

    #[test]
    fn depth_snapshot() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(101_00), 150, TimeInForce::GTC);

        let snap = exchange.depth(10);

        assert_eq!(snap.bids.len(), 2);
        assert_eq!(snap.asks.len(), 1);
        assert_eq!(snap.best_bid(), Some(Price(100_00)));
        assert_eq!(snap.best_ask(), Some(Price(101_00)));
    }
}
