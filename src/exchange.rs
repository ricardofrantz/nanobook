//! Exchange: The high-level API for order submission and management.
//!
//! This is the main entry point for users of the library. It wraps the
//! OrderBook and provides methods for submitting orders with proper
//! time-in-force handling.

#[cfg(feature = "event-log")]
use crate::event::Event;
use crate::{
    error::ValidationError,
    result::{CancelError, CancelResult, ModifyError, ModifyResult, StopSubmitResult, SubmitResult},
    snapshot::BookSnapshot,
    stop::{StopBook, StopOrder, StopStatus, TrailMethod},
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
    /// Stop order book
    pub(crate) stop_book: StopBook,
    /// Last trade price (for stop order triggers)
    pub(crate) last_trade_price: Option<Price>,
    /// Event log for replay (only with "event-log" feature)
    #[cfg(feature = "event-log")]
    pub(crate) events: Vec<crate::event::Event>,
}

impl Exchange {
    /// Create a new exchange with an empty order book.
    pub fn new() -> Self {
        Self {
            book: OrderBook::new(),
            trades: Vec::new(),
            stop_book: StopBook::new(),
            last_trade_price: None,
            #[cfg(feature = "event-log")]
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
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitLimit {
            side,
            price,
            quantity,
            time_in_force: tif,
        });

        let result = self.submit_limit_internal(side, price, quantity, tif);
        if !result.trades.is_empty() {
            let last_price = result.trades.last().unwrap().price;
            self.last_trade_price = Some(last_price);
            self.process_trade_triggers();
        }
        result
    }

    /// Submit a market order.
    ///
    /// Market orders execute immediately at the best available prices.
    /// Any unfilled quantity is cancelled (IOC semantics).
    ///
    /// This is equivalent to a limit order at the worst possible price
    /// with IOC time-in-force.
    pub fn submit_market(&mut self, side: Side, quantity: Quantity) -> SubmitResult {
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitMarket { side, quantity });

        // Market order = limit at worst price + IOC
        let price = match side {
            Side::Buy => Price::MAX,
            Side::Sell => Price::MIN,
        };
        let result = self.submit_limit_internal(side, price, quantity, TimeInForce::IOC);
        if !result.trades.is_empty() {
            let last_price = result.trades.last().unwrap().price;
            self.last_trade_price = Some(last_price);
            self.process_trade_triggers();
        }
        result
    }

    /// Submit a limit order with input validation.
    ///
    /// Returns `Err(ValidationError::ZeroQuantity)` if quantity is 0,
    /// or `Err(ValidationError::ZeroPrice)` if price is <= 0.
    pub fn try_submit_limit(
        &mut self,
        side: Side,
        price: Price,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> Result<SubmitResult, ValidationError> {
        if quantity == 0 {
            return Err(ValidationError::ZeroQuantity);
        }
        if price.0 <= 0 {
            return Err(ValidationError::ZeroPrice);
        }
        Ok(self.submit_limit(side, price, quantity, tif))
    }

    /// Submit a market order with input validation.
    ///
    /// Returns `Err(ValidationError::ZeroQuantity)` if quantity is 0.
    pub fn try_submit_market(
        &mut self,
        side: Side,
        quantity: Quantity,
    ) -> Result<SubmitResult, ValidationError> {
        if quantity == 0 {
            return Err(ValidationError::ZeroQuantity);
        }
        Ok(self.submit_market(side, quantity))
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
        if tif == TimeInForce::FOK && !self.book.can_fully_fill(side, price, quantity) {
            // Reject the order. We still consume an OrderId for consistency
            // (the caller gets a valid ID even for rejected orders).
            // Note: This creates gaps in the OrderId sequence for rejected FOKs,
            // and the order is not stored (get_order returns None).
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
            // Fully filled
            order.status = OrderStatus::Filled;
            self.book.orders.insert(order_id, order);
            (OrderStatus::Filled, 0, 0)
        } else if tif == TimeInForce::GTC {
            // Rest on book
            let status = if filled > 0 {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::New
            };
            order.status = status;
            self.book.add_order(order);
            (status, remaining, 0)
        } else {
            // IOC/FOK: cancel remainder (FOK shouldn't reach here with remainder)
            let status = if filled > 0 {
                OrderStatus::PartiallyFilled
            } else {
                OrderStatus::Cancelled
            };
            order.status = status;
            self.book.orders.insert(order_id, order);
            (status, 0, remaining)
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
        #[cfg(feature = "event-log")]
        self.events.push(Event::Cancel { order_id });

        self.cancel_internal(order_id)
    }

    /// Internal: cancel without recording event.
    pub(crate) fn cancel_internal(&mut self, order_id: OrderId) -> CancelResult {
        // Check stop book first
        if self.stop_book.contains_pending(order_id) {
            if let Some(stop) = self.stop_book.get(order_id) {
                let qty = stop.quantity;
                self.stop_book.cancel(order_id);
                return CancelResult::success(qty);
            }
        }

        // Check if order exists in regular book
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
        #[cfg(feature = "event-log")]
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

    // === Stop Orders ===

    /// Maximum cascade depth to prevent infinite stop-trigger loops.
    const MAX_CASCADE_DEPTH: usize = 100;

    /// Submit a stop-market order.
    ///
    /// The order becomes a market order when `last_trade_price` reaches `stop_price`.
    /// - Buy stop: triggers when `last_trade_price >= stop_price`
    /// - Sell stop: triggers when `last_trade_price <= stop_price`
    pub fn submit_stop_market(
        &mut self,
        side: Side,
        stop_price: Price,
        quantity: Quantity,
    ) -> StopSubmitResult {
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitStopMarket {
            side,
            stop_price,
            quantity,
        });

        self.submit_stop_internal(side, stop_price, None, quantity, TimeInForce::GTC)
    }

    /// Submit a stop-limit order.
    ///
    /// The order becomes a limit order at `limit_price` when `last_trade_price`
    /// reaches `stop_price`.
    pub fn submit_stop_limit(
        &mut self,
        side: Side,
        stop_price: Price,
        limit_price: Price,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> StopSubmitResult {
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitStopLimit {
            side,
            stop_price,
            limit_price,
            quantity,
            time_in_force: tif,
        });

        self.submit_stop_internal(side, stop_price, Some(limit_price), quantity, tif)
    }

    /// Submit a trailing stop-market order.
    ///
    /// The stop price adjusts as the market moves favorably:
    /// - Sell trailing: stop follows the market UP (protects long positions)
    /// - Buy trailing: stop follows the market DOWN (protects short positions)
    ///
    /// `initial_stop_price` is the starting stop price before any trailing.
    pub fn submit_trailing_stop_market(
        &mut self,
        side: Side,
        initial_stop_price: Price,
        quantity: Quantity,
        trail_method: TrailMethod,
    ) -> StopSubmitResult {
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitTrailingStopMarket {
            side,
            stop_price: initial_stop_price,
            quantity,
            trail_method: trail_method.clone(),
        });

        self.submit_trailing_stop_internal(
            side,
            initial_stop_price,
            None,
            quantity,
            TimeInForce::GTC,
            trail_method,
        )
    }

    /// Submit a trailing stop-limit order.
    ///
    /// Like a trailing stop-market, but when triggered becomes a limit order
    /// at `limit_price`.
    pub fn submit_trailing_stop_limit(
        &mut self,
        side: Side,
        initial_stop_price: Price,
        limit_price: Price,
        quantity: Quantity,
        tif: TimeInForce,
        trail_method: TrailMethod,
    ) -> StopSubmitResult {
        #[cfg(feature = "event-log")]
        self.events.push(Event::SubmitTrailingStopLimit {
            side,
            stop_price: initial_stop_price,
            limit_price,
            quantity,
            time_in_force: tif,
            trail_method: trail_method.clone(),
        });

        self.submit_trailing_stop_internal(
            side,
            initial_stop_price,
            Some(limit_price),
            quantity,
            tif,
            trail_method,
        )
    }

    /// Internal: submit trailing stop order.
    pub(crate) fn submit_trailing_stop_internal(
        &mut self,
        side: Side,
        stop_price: Price,
        limit_price: Option<Price>,
        quantity: Quantity,
        tif: TimeInForce,
        trail_method: TrailMethod,
    ) -> StopSubmitResult {
        self.insert_stop_order(side, stop_price, limit_price, quantity, tif, Some(trail_method))
    }

    /// Internal: submit stop order without recording event.
    pub(crate) fn submit_stop_internal(
        &mut self,
        side: Side,
        stop_price: Price,
        limit_price: Option<Price>,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> StopSubmitResult {
        self.insert_stop_order(side, stop_price, limit_price, quantity, tif, None)
    }

    /// Shared logic for inserting stop/trailing-stop orders.
    fn insert_stop_order(
        &mut self,
        side: Side,
        stop_price: Price,
        limit_price: Option<Price>,
        quantity: Quantity,
        tif: TimeInForce,
        trail_method: Option<TrailMethod>,
    ) -> StopSubmitResult {
        let id = self.book.next_order_id();
        let timestamp = self.book.next_timestamp();
        let is_trailing = trail_method.is_some();

        let order = StopOrder {
            id,
            side,
            stop_price,
            limit_price,
            quantity,
            time_in_force: tif,
            timestamp,
            status: StopStatus::Pending,
            trail_method,
            watermark: None,
        };

        self.stop_book.insert(order);

        // Trailing stops don't trigger immediately — they need price movement to
        // establish the watermark first. update_trailing_stops() will adjust the
        // stop price relative to the watermark, so the raw stop_price check would
        // be misleading.
        if !is_trailing {
            if let Some(last_price) = self.last_trade_price {
                let should_trigger = match side {
                    Side::Buy => last_price >= stop_price,
                    Side::Sell => last_price <= stop_price,
                };
                if should_trigger {
                    self.process_trade_triggers();
                    let status = self
                        .stop_book
                        .get(id)
                        .map(|o| o.status)
                        .unwrap_or(StopStatus::Triggered);
                    return StopSubmitResult {
                        order_id: id,
                        status,
                    };
                }
            }
        }

        StopSubmitResult {
            order_id: id,
            status: StopStatus::Pending,
        }
    }

    /// Process stop order triggers after trades occur.
    ///
    /// Trailing stops are updated BEFORE checking triggers, so their
    /// stop prices reflect the latest market move.
    ///
    /// Triggered stops may produce trades that trigger more stops (cascade).
    /// Limited to `MAX_CASCADE_DEPTH` iterations to prevent infinite loops.
    pub(crate) fn process_trade_triggers(&mut self) {
        for _ in 0..Self::MAX_CASCADE_DEPTH {
            let trade_price = match self.last_trade_price {
                Some(p) => p,
                None => return,
            };

            // Update trailing stops before checking triggers
            self.stop_book.update_trailing_stops(trade_price);

            let triggered = self.stop_book.collect_triggered(trade_price);
            if triggered.is_empty() {
                return;
            }

            let mut new_last_price = None;

            for stop in triggered {
                let result = match stop.limit_price {
                    Some(limit) => {
                        self.submit_limit_internal(stop.side, limit, stop.quantity, stop.time_in_force)
                    }
                    None => {
                        let price = match stop.side {
                            Side::Buy => Price::MAX,
                            Side::Sell => Price::MIN,
                        };
                        self.submit_limit_internal(stop.side, price, stop.quantity, TimeInForce::IOC)
                    }
                };

                // submit_limit_internal already records trades in self.trades
                if let Some(last_trade) = result.trades.last() {
                    new_last_price = Some(last_trade.price);
                }
            }

            match new_last_price {
                Some(p) => self.last_trade_price = Some(p),
                None => return, // No new trades, no more triggers possible
            }
        }
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

    /// Get a stop order by ID.
    pub fn get_stop_order(&self, order_id: OrderId) -> Option<&StopOrder> {
        self.stop_book.get(order_id)
    }

    /// Get the number of pending stop orders.
    pub fn pending_stop_count(&self) -> usize {
        self.stop_book.pending_count()
    }

    /// Get the last trade price.
    pub fn last_trade_price(&self) -> Option<Price> {
        self.last_trade_price
    }

    /// Get the stop book (for advanced queries).
    pub fn stop_book(&self) -> &StopBook {
        &self.stop_book
    }

    // === Memory Management ===

    /// Clear trade history to free memory.
    ///
    /// Use periodically for long-running instances.
    pub fn clear_trades(&mut self) {
        self.trades.clear();
    }

    /// Remove filled and cancelled orders from history.
    ///
    /// Active orders (on the book) are preserved. Returns the number
    /// of orders removed. Also clears triggered/cancelled stop orders.
    pub fn clear_order_history(&mut self) -> usize {
        self.stop_book.clear_history();
        self.book.clear_history()
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

    // === Validation ===

    #[test]
    fn try_submit_limit_zero_quantity() {
        let mut exchange = Exchange::new();
        let result = exchange.try_submit_limit(Side::Buy, Price(100_00), 0, TimeInForce::GTC);
        assert_eq!(result.unwrap_err(), ValidationError::ZeroQuantity);
    }

    #[test]
    fn try_submit_limit_zero_price() {
        let mut exchange = Exchange::new();
        let result = exchange.try_submit_limit(Side::Buy, Price(0), 100, TimeInForce::GTC);
        assert_eq!(result.unwrap_err(), ValidationError::ZeroPrice);
    }

    #[test]
    fn try_submit_limit_negative_price() {
        let mut exchange = Exchange::new();
        let result = exchange.try_submit_limit(Side::Buy, Price(-100), 100, TimeInForce::GTC);
        assert_eq!(result.unwrap_err(), ValidationError::ZeroPrice);
    }

    #[test]
    fn try_submit_limit_valid() {
        let mut exchange = Exchange::new();
        let result = exchange.try_submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().order_id, OrderId(1));
    }

    #[test]
    fn try_submit_market_zero_quantity() {
        let mut exchange = Exchange::new();
        let result = exchange.try_submit_market(Side::Buy, 0);
        assert_eq!(result.unwrap_err(), ValidationError::ZeroQuantity);
    }

    #[test]
    fn try_submit_market_valid() {
        let mut exchange = Exchange::new();
        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.try_submit_market(Side::Buy, 50);
        assert!(result.is_ok());
    }

    // === Stop Orders ===

    #[test]
    fn submit_stop_market_pending() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_stop_market(Side::Buy, Price(105_00), 100);
        assert_eq!(result.status, StopStatus::Pending);
        assert_eq!(exchange.pending_stop_count(), 1);
    }

    #[test]
    fn stop_market_triggers_on_trade() {
        let mut exchange = Exchange::new();

        // Set up a resting ask
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        // Set up asks for the triggered order to fill against
        exchange.submit_limit(Side::Sell, Price(105_00), 100, TimeInForce::GTC);

        // Place buy stop at 100
        exchange.submit_stop_market(Side::Buy, Price(100_00), 100);

        // Now submit a buy that crosses the ask and produces a trade at 100
        let result = exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);
        assert_eq!(result.trades.len(), 1);

        // Stop should have triggered and filled against the 105 ask
        assert_eq!(exchange.pending_stop_count(), 0);
        assert_eq!(exchange.last_trade_price(), Some(Price(105_00)));
    }

    #[test]
    fn stop_limit_triggers_with_limit_price() {
        let mut exchange = Exchange::new();

        // Set up asks
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(106_00), 100, TimeInForce::GTC);

        // Place buy stop-limit: triggers at 100, but only buy up to 105
        exchange.submit_stop_limit(
            Side::Buy,
            Price(100_00),
            Price(105_00),
            100,
            TimeInForce::GTC,
        );

        // Trigger with a trade at 100
        exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);

        // Stop triggered, but limit price 105 doesn't cross ask at 106
        // So it should rest on the book
        assert_eq!(exchange.pending_stop_count(), 0);
        assert_eq!(exchange.best_bid(), Some(Price(105_00)));
    }

    #[test]
    fn cancel_stop_order() {
        let mut exchange = Exchange::new();

        let stop = exchange.submit_stop_market(Side::Buy, Price(105_00), 100);
        assert_eq!(exchange.pending_stop_count(), 1);

        let result = exchange.cancel(stop.order_id);
        assert!(result.success);
        assert_eq!(result.cancelled_quantity, 100);
        assert_eq!(exchange.pending_stop_count(), 0);
    }

    #[test]
    fn sell_stop_triggers_on_price_drop() {
        let mut exchange = Exchange::new();

        // Set up a resting bid to establish a price
        exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);
        // Set up bids for the triggered sell to fill against
        exchange.submit_limit(Side::Buy, Price(95_00), 100, TimeInForce::GTC);

        // Sell stop at 100: triggers when price drops to 100
        exchange.submit_stop_market(Side::Sell, Price(100_00), 100);

        // Trade at 100 triggers the sell stop
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);

        assert_eq!(exchange.pending_stop_count(), 0);
    }

    #[test]
    fn immediate_trigger_if_price_already_past() {
        let mut exchange = Exchange::new();

        // Create a trade to establish last_trade_price at 100
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);
        assert_eq!(exchange.last_trade_price(), Some(Price(100_00)));

        // Set up more asks for the stop to fill against
        exchange.submit_limit(Side::Sell, Price(105_00), 100, TimeInForce::GTC);

        // Submit buy stop at 99 — already past, should trigger immediately
        let result = exchange.submit_stop_market(Side::Buy, Price(99_00), 100);
        assert_eq!(result.status, StopStatus::Triggered);
        assert_eq!(exchange.pending_stop_count(), 0);
    }

    #[test]
    fn stop_cascade() {
        let mut exchange = Exchange::new();

        // Set up asks at different levels
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(102_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(104_00), 50, TimeInForce::GTC);

        // Buy stop at 100 — when triggered, will trade at 102
        exchange.submit_stop_market(Side::Buy, Price(100_00), 50);
        // Buy stop at 102 — cascading trigger from first stop's trade
        exchange.submit_stop_market(Side::Buy, Price(102_00), 50);

        // Trigger cascade: trade at 100 -> stop1 triggers -> trades at 102 -> stop2 triggers
        exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);

        assert_eq!(exchange.pending_stop_count(), 0);
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

    // === Trailing Stop Orders ===

    #[test]
    fn trailing_stop_market_sell() {
        let mut exchange = Exchange::new();

        // Set up order book: asks at 100 and bids at 90 for the triggered sell
        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(90_00), 200, TimeInForce::GTC);

        // Place trailing sell stop: initial stop at 95, trail by $3
        let result = exchange.submit_trailing_stop_market(
            Side::Sell,
            Price(95_00),
            100,
            TrailMethod::Fixed(3_00),
        );
        assert_eq!(result.status, StopStatus::Pending);

        // Trade at 100 (buy crosses the ask) — watermark should move up
        exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        // The trailing stop should have adjusted: watermark=100, stop=97
        // It should not have triggered (price 100 > stop 97 for sell)
        let stop = exchange.get_stop_order(result.order_id).unwrap();
        assert_eq!(stop.watermark, Some(Price(100_00)));
        assert_eq!(stop.stop_price, Price(97_00));
    }

    #[test]
    fn trailing_stop_triggers_on_reversal() {
        let mut exchange = Exchange::new();

        // Build book: asks and bids for trading
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(105_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(90_00), 200, TimeInForce::GTC);

        // Place trailing sell stop: initial stop at 98, trail by $2
        exchange.submit_trailing_stop_market(
            Side::Sell,
            Price(98_00),
            50,
            TrailMethod::Fixed(2_00),
        );

        // Trade at 100 — trailing updates to stop=98, watermark=100
        exchange.submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC);
        assert_eq!(exchange.pending_stop_count(), 1);

        // Trade at 105 — trailing updates to stop=103, watermark=105
        exchange.submit_limit(Side::Buy, Price(105_00), 50, TimeInForce::GTC);

        // Now set up a sell at 103 and buy at 90 to drop the price
        exchange.submit_limit(Side::Buy, Price(103_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Sell, Price(103_00), 50, TimeInForce::GTC);
        // Trade at 103 should trigger the trailing stop (stop_price=103)
        assert_eq!(exchange.pending_stop_count(), 0);
    }

    #[test]
    fn trailing_stop_percentage_method() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(80_00), 200, TimeInForce::GTC);

        // Trailing sell stop: 5% trailing distance
        let result = exchange.submit_trailing_stop_market(
            Side::Sell,
            Price(90_00),
            50,
            TrailMethod::Percentage(0.05),
        );
        assert_eq!(result.status, StopStatus::Pending);

        // Trade at 100 — watermark=100, offset=5% of 100 = $5, stop=95
        exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let stop = exchange.get_stop_order(result.order_id).unwrap();
        assert_eq!(stop.watermark, Some(Price(100_00)));
        assert_eq!(stop.stop_price, Price(95_00));
    }

    #[test]
    fn trailing_stop_does_not_trigger_immediately() {
        let mut exchange = Exchange::new();

        // Establish last_trade_price at 90 via a trade
        exchange.submit_limit(Side::Sell, Price(90_00), 50, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(90_00), 50, TimeInForce::GTC);
        assert_eq!(exchange.last_trade_price(), Some(Price(90_00)));

        // Submit trailing sell stop with stop_price=95 — although 90 <= 95,
        // trailing stops wait for price movement to establish the watermark first
        let result = exchange.submit_trailing_stop_market(
            Side::Sell,
            Price(95_00),
            50,
            TrailMethod::Fixed(3_00),
        );
        assert_eq!(result.status, StopStatus::Pending);
        assert_eq!(exchange.pending_stop_count(), 1);
    }

    #[test]
    fn cancel_trailing_stop() {
        let mut exchange = Exchange::new();

        let result = exchange.submit_trailing_stop_market(
            Side::Sell,
            Price(95_00),
            100,
            TrailMethod::Fixed(3_00),
        );

        let cancel = exchange.cancel(result.order_id);
        assert!(cancel.success);
        assert_eq!(exchange.pending_stop_count(), 0);
    }
}
