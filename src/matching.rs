//! Matching engine: the core algorithm for executing trades.
//!
//! The matching engine implements price-time priority:
//! 1. Better prices match first (higher bids, lower asks)
//! 2. At the same price, earlier orders match first (FIFO)
//! 3. Trades execute at the resting order's price (price improvement for aggressor)

use crate::{Order, OrderBook, Price, Quantity, Side, Trade};

/// Result of matching an incoming order against the book.
#[derive(Clone, Debug, Default)]
pub struct MatchResult {
    /// Trades that occurred during matching
    pub trades: Vec<Trade>,
    /// Quantity that could not be filled
    pub remaining_quantity: Quantity,
}

impl MatchResult {
    /// Returns the total quantity filled.
    pub fn filled_quantity(&self) -> Quantity {
        self.trades.iter().map(|t| t.quantity).sum()
    }

    /// Returns true if the order was fully filled.
    pub fn is_fully_filled(&self) -> bool {
        self.remaining_quantity == 0
    }

    /// Returns true if no trades occurred.
    pub fn is_empty(&self) -> bool {
        self.trades.is_empty()
    }
}

impl OrderBook {
    /// Check if an incoming order's price crosses with a resting price.
    ///
    /// - Buy crosses if buy_price >= ask_price
    /// - Sell crosses if sell_price <= bid_price
    #[inline]
    fn prices_cross(incoming_side: Side, incoming_price: Price, resting_price: Price) -> bool {
        match incoming_side {
            Side::Buy => incoming_price >= resting_price,
            Side::Sell => incoming_price <= resting_price,
        }
    }

    /// Match an incoming order against the book.
    ///
    /// This is the core matching algorithm:
    /// 1. Find the best price on the opposite side
    /// 2. If prices cross, fill against resting orders (FIFO)
    /// 3. Continue until no more crosses or order is filled
    /// 4. Return trades and remaining quantity
    ///
    /// **Important**: This method modifies both the incoming order and
    /// resting orders in the book. The incoming order is NOT added to
    /// the book — the caller decides whether to add it based on TIF.
    pub fn match_order(&mut self, incoming: &mut Order) -> MatchResult {
        let mut result = MatchResult {
            trades: Vec::new(),
            remaining_quantity: incoming.remaining_quantity,
        };

        // Match until no more crosses or order is filled
        while incoming.remaining_quantity > 0 {
            // Get the best price on the opposite side
            let opposite = self.opposite_side(incoming.side);
            let best_price = match opposite.best_price() {
                Some(p) => p,
                None => break, // No liquidity
            };

            // Check if prices cross
            if !Self::prices_cross(incoming.side, incoming.price, best_price) {
                break; // No match at this price
            }

            // Match against orders at the best price level
            self.match_at_price(incoming, best_price, &mut result);
        }

        result.remaining_quantity = incoming.remaining_quantity;
        result
    }

    /// Match an incoming order against all orders at a specific price level.
    fn match_at_price(&mut self, incoming: &mut Order, price: Price, result: &mut MatchResult) {
        // Process orders at this price level until exhausted or incoming filled
        while incoming.remaining_quantity > 0 {
            // Get the front order at this price
            let opposite = self.opposite_side(incoming.side);
            let resting_id = match opposite.best_level().and_then(|l| l.front()) {
                Some(id) if opposite.best_price() == Some(price) => id,
                _ => break, // Level exhausted or price changed
            };

            // Get the resting order's remaining quantity
            let resting_remaining = match self.get_order(resting_id) {
                Some(o) => o.remaining_quantity,
                None => {
                    // Orphaned order ID in level — shouldn't happen, but handle gracefully
                    self.opposite_side_mut(incoming.side)
                        .get_level_mut(price)
                        .map(|l| l.pop_front(0));
                    continue;
                }
            };

            // Calculate fill quantity
            let fill_qty = incoming.remaining_quantity.min(resting_remaining);

            // Create the trade
            let trade = Trade::new(
                self.next_trade_id(),
                price, // Trade at resting order's price
                fill_qty,
                incoming.id,
                resting_id,
                incoming.side,
                self.next_timestamp(),
            );
            result.trades.push(trade);

            // Update the incoming order
            incoming.fill(fill_qty);

            // Update the resting order
            let resting_fully_filled = {
                let resting = self.get_order_mut(resting_id).unwrap();
                resting.fill(fill_qty);
                resting.remaining_quantity == 0
            };

            // Update the price level
            let opposite = self.opposite_side_mut(incoming.side);
            if resting_fully_filled {
                // Remove the fully filled order from the level
                if let Some(level) = opposite.get_level_mut(price) {
                    level.pop_front(fill_qty);
                    if level.is_empty() {
                        opposite.remove_level(price);
                    }
                }
            } else {
                // Just decrease the level's quantity
                if let Some(level) = opposite.get_level_mut(price) {
                    level.decrease_quantity(fill_qty);
                }
            }
        }
    }

    /// Calculate how much quantity is available at prices that would cross.
    ///
    /// This is used for FOK (fill-or-kill) feasibility checks.
    pub fn available_to_fill(&self, side: Side, price: Price) -> Quantity {
        self.opposite_side(side).quantity_at_or_better(price)
    }

    /// Check if an order can be fully filled (for FOK orders).
    pub fn can_fully_fill(&self, side: Side, price: Price, quantity: Quantity) -> bool {
        self.available_to_fill(side, price) >= quantity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{OrderId, OrderStatus, TimeInForce};

    /// Helper to create a book with some resting orders
    fn book_with_asks(asks: &[(i64, u64)]) -> OrderBook {
        let mut book = OrderBook::new();
        for &(price, qty) in asks {
            let order = book.create_order(Side::Sell, Price(price), qty, TimeInForce::GTC);
            book.add_order(order);
        }
        book
    }

    fn book_with_bids(bids: &[(i64, u64)]) -> OrderBook {
        let mut book = OrderBook::new();
        for &(price, qty) in bids {
            let order = book.create_order(Side::Buy, Price(price), qty, TimeInForce::GTC);
            book.add_order(order);
        }
        book
    }

    // === No match scenarios ===

    #[test]
    fn no_match_empty_book() {
        let mut book = OrderBook::new();
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        assert!(result.is_empty());
        assert_eq!(result.remaining_quantity, 100);
        assert!(!result.is_fully_filled());
    }

    #[test]
    fn no_match_prices_dont_cross() {
        let mut book = book_with_asks(&[(101_00, 100)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        assert!(result.is_empty());
        assert_eq!(result.remaining_quantity, 100);
        // Ask still on book
        assert_eq!(book.best_ask(), Some(Price(101_00)));
    }

    // === Full fill scenarios ===

    #[test]
    fn full_fill_exact_quantity() {
        let mut book = book_with_asks(&[(100_00, 100)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.filled_quantity(), 100);
        assert!(result.is_fully_filled());

        // Verify trade details
        let trade = &result.trades[0];
        assert_eq!(trade.price, Price(100_00));
        assert_eq!(trade.quantity, 100);
        assert_eq!(trade.aggressor_side, Side::Buy);

        // Book should be empty on ask side
        assert_eq!(book.best_ask(), None);

        // Resting order should be filled
        let resting = book.get_order(OrderId(1)).unwrap();
        assert_eq!(resting.status, OrderStatus::Filled);
    }

    #[test]
    fn full_fill_incoming_smaller() {
        let mut book = book_with_asks(&[(100_00, 200)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        assert_eq!(result.trades.len(), 1);
        assert!(result.is_fully_filled());

        // Resting order partially filled, still on book
        assert_eq!(book.best_ask(), Some(Price(100_00)));
        let resting = book.get_order(OrderId(1)).unwrap();
        assert_eq!(resting.remaining_quantity, 100);
        assert_eq!(resting.status, OrderStatus::PartiallyFilled);
    }

    // === Partial fill scenarios ===

    #[test]
    fn partial_fill_incoming_larger() {
        let mut book = book_with_asks(&[(100_00, 50)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.filled_quantity(), 50);
        assert_eq!(result.remaining_quantity, 50);
        assert!(!result.is_fully_filled());

        // Resting order fully consumed
        assert_eq!(book.best_ask(), None);
    }

    // === Multi-order matching (FIFO) ===

    #[test]
    fn fifo_same_price() {
        // Three asks at same price
        let mut book = book_with_asks(&[(100_00, 30), (100_00, 40), (100_00, 50)]);

        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = book.match_order(&mut order);

        // Should fill against first two orders completely, third partially
        assert_eq!(result.trades.len(), 3);
        assert_eq!(result.trades[0].quantity, 30); // First order
        assert_eq!(result.trades[1].quantity, 40); // Second order
        assert_eq!(result.trades[2].quantity, 30); // Third order (partial)
        assert!(result.is_fully_filled());

        // First two orders filled, third partially filled
        assert_eq!(book.get_order(OrderId(1)).unwrap().status, OrderStatus::Filled);
        assert_eq!(book.get_order(OrderId(2)).unwrap().status, OrderStatus::Filled);
        assert_eq!(book.get_order(OrderId(3)).unwrap().status, OrderStatus::PartiallyFilled);
        assert_eq!(book.get_order(OrderId(3)).unwrap().remaining_quantity, 20);
    }

    // === Multi-level matching (price priority) ===

    #[test]
    fn price_priority_buy_sweeps_asks() {
        // Asks at different prices
        let mut book = book_with_asks(&[(100_00, 50), (101_00, 50), (102_00, 50)]);

        // Buy at 102 should sweep through all prices
        let mut order = book.create_order(Side::Buy, Price(102_00), 120, TimeInForce::GTC);
        let result = book.match_order(&mut order);

        assert_eq!(result.trades.len(), 3);
        // Fills at best prices first (lowest asks)
        assert_eq!(result.trades[0].price, Price(100_00));
        assert_eq!(result.trades[0].quantity, 50);
        assert_eq!(result.trades[1].price, Price(101_00));
        assert_eq!(result.trades[1].quantity, 50);
        assert_eq!(result.trades[2].price, Price(102_00));
        assert_eq!(result.trades[2].quantity, 20);

        assert!(result.is_fully_filled());

        // Only 30 left at 102
        assert_eq!(book.best_ask(), Some(Price(102_00)));
        assert_eq!(book.asks().total_quantity(), 30);
    }

    #[test]
    fn price_priority_sell_sweeps_bids() {
        // Bids at different prices
        let mut book = book_with_bids(&[(100_00, 50), (99_00, 50), (98_00, 50)]);

        // Sell at 98 should sweep through all prices
        let mut order = book.create_order(Side::Sell, Price(98_00), 120, TimeInForce::GTC);
        let result = book.match_order(&mut order);

        assert_eq!(result.trades.len(), 3);
        // Fills at best prices first (highest bids)
        assert_eq!(result.trades[0].price, Price(100_00));
        assert_eq!(result.trades[1].price, Price(99_00));
        assert_eq!(result.trades[2].price, Price(98_00));

        assert!(result.is_fully_filled());
    }

    // === Price improvement ===

    #[test]
    fn price_improvement_for_buyer() {
        // Ask at 100, buyer willing to pay 105
        let mut book = book_with_asks(&[(100_00, 100)]);
        let mut order = book.create_order(Side::Buy, Price(105_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        // Trade executes at 100 (resting price), not 105 (incoming limit)
        assert_eq!(result.trades[0].price, Price(100_00));
    }

    #[test]
    fn price_improvement_for_seller() {
        // Bid at 105, seller willing to accept 100
        let mut book = book_with_bids(&[(105_00, 100)]);
        let mut order = book.create_order(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        let result = book.match_order(&mut order);

        // Trade executes at 105 (resting price), not 100 (incoming limit)
        assert_eq!(result.trades[0].price, Price(105_00));
    }

    // === Order state after matching ===

    #[test]
    fn incoming_order_state_after_full_fill() {
        let mut book = book_with_asks(&[(100_00, 100)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        book.match_order(&mut order);

        assert_eq!(order.remaining_quantity, 0);
        assert_eq!(order.filled_quantity, 100);
        assert_eq!(order.status, OrderStatus::Filled);
    }

    #[test]
    fn incoming_order_state_after_partial_fill() {
        let mut book = book_with_asks(&[(100_00, 30)]);
        let mut order = book.create_order(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        book.match_order(&mut order);

        assert_eq!(order.remaining_quantity, 70);
        assert_eq!(order.filled_quantity, 30);
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
    }

    // === FOK feasibility checks ===

    #[test]
    fn available_to_fill() {
        let book = book_with_asks(&[(100_00, 50), (101_00, 75), (102_00, 100)]);

        // Buy at 100: only 50 available
        assert_eq!(book.available_to_fill(Side::Buy, Price(100_00)), 50);

        // Buy at 101: 50 + 75 = 125 available
        assert_eq!(book.available_to_fill(Side::Buy, Price(101_00)), 125);

        // Buy at 102: all 225 available
        assert_eq!(book.available_to_fill(Side::Buy, Price(102_00)), 225);
    }

    #[test]
    fn can_fully_fill() {
        let book = book_with_asks(&[(100_00, 100)]);

        assert!(book.can_fully_fill(Side::Buy, Price(100_00), 50));
        assert!(book.can_fully_fill(Side::Buy, Price(100_00), 100));
        assert!(!book.can_fully_fill(Side::Buy, Price(100_00), 101));
        assert!(!book.can_fully_fill(Side::Buy, Price(99_00), 50)); // Price doesn't cross
    }

    // === Edge cases ===

    #[test]
    fn match_clears_multiple_levels() {
        let mut book = book_with_asks(&[(100_00, 10), (101_00, 10)]);

        let mut order = book.create_order(Side::Buy, Price(101_00), 20, TimeInForce::GTC);
        book.match_order(&mut order);

        // Both levels should be cleared
        assert_eq!(book.asks().level_count(), 0);
        assert_eq!(book.best_ask(), None);
    }

    #[test]
    fn trade_ids_are_sequential() {
        let mut book = book_with_asks(&[(100_00, 30), (100_00, 30), (100_00, 30)]);

        let mut order = book.create_order(Side::Buy, Price(100_00), 90, TimeInForce::GTC);
        let result = book.match_order(&mut order);

        // Trade IDs should be sequential
        assert_eq!(result.trades[0].id.0, 1);
        assert_eq!(result.trades[1].id.0, 2);
        assert_eq!(result.trades[2].id.0, 3);
    }

    #[test]
    fn timestamps_are_sequential() {
        let mut book = book_with_asks(&[(100_00, 30), (100_00, 30)]);

        let mut order = book.create_order(Side::Buy, Price(100_00), 60, TimeInForce::GTC);
        let result = book.match_order(&mut order);

        // Timestamps should be sequential (after order creation timestamps)
        assert!(result.trades[0].timestamp < result.trades[1].timestamp);
    }
}
