//! Event log for deterministic replay.
//!
//! All inputs to the exchange are recorded as events, enabling:
//! - Exact state reconstruction via replay
//! - Backtesting with historical data
//! - Debugging and audit trails
//! - Serialization/persistence of exchange state

#[cfg(feature = "event-log")]
use crate::Exchange;
use crate::{OrderId, Price, Quantity, Side, TimeInForce, Trade};

/// An event that can be applied to an exchange.
///
/// Events capture all inputs (not outputs like trades).
/// Replaying the same events produces identical state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// Submit a limit order
    SubmitLimit {
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
    },
    /// Submit a market order
    SubmitMarket { side: Side, quantity: Quantity },
    /// Cancel an order
    Cancel { order_id: OrderId },
    /// Modify an order (cancel and replace)
    Modify {
        order_id: OrderId,
        new_price: Price,
        new_quantity: Quantity,
    },
}

impl Event {
    /// Create a SubmitLimit event.
    pub fn submit_limit(
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
    ) -> Self {
        Event::SubmitLimit {
            side,
            price,
            quantity,
            time_in_force,
        }
    }

    /// Create a SubmitMarket event.
    pub fn submit_market(side: Side, quantity: Quantity) -> Self {
        Event::SubmitMarket { side, quantity }
    }

    /// Create a Cancel event.
    pub fn cancel(order_id: OrderId) -> Self {
        Event::Cancel { order_id }
    }

    /// Create a Modify event.
    pub fn modify(order_id: OrderId, new_price: Price, new_quantity: Quantity) -> Self {
        Event::Modify {
            order_id,
            new_price,
            new_quantity,
        }
    }
}

/// Result of applying an event.
#[derive(Clone, Debug)]
pub struct ApplyResult {
    /// Trades that occurred (if any)
    pub trades: Vec<Trade>,
}

#[cfg(feature = "event-log")]
impl Exchange {
    /// Apply a single event to the exchange.
    ///
    /// Returns any trades that resulted from the event.
    /// Events applied via this method are recorded in the event log.
    pub fn apply(&mut self, event: &Event) -> ApplyResult {
        // Record the event
        self.events.push(event.clone());

        // Apply using internal methods (no double recording)
        let trades = match event {
            Event::SubmitLimit {
                side,
                price,
                quantity,
                time_in_force,
            } => {
                let result = self.submit_limit_internal(*side, *price, *quantity, *time_in_force);
                result.trades
            }
            Event::SubmitMarket { side, quantity } => {
                let price = match side {
                    crate::Side::Buy => crate::Price::MAX,
                    crate::Side::Sell => crate::Price::MIN,
                };
                let result =
                    self.submit_limit_internal(*side, price, *quantity, crate::TimeInForce::IOC);
                result.trades
            }
            Event::Cancel { order_id } => {
                self.cancel_internal(*order_id);
                Vec::new()
            }
            Event::Modify {
                order_id,
                new_price,
                new_quantity,
            } => {
                let result = self.modify_internal(*order_id, *new_price, *new_quantity);
                result.trades
            }
        };

        ApplyResult { trades }
    }

    /// Apply multiple events in sequence.
    ///
    /// Returns all trades that occurred.
    pub fn apply_all(&mut self, events: &[Event]) -> Vec<Trade> {
        events.iter().flat_map(|e| self.apply(e).trades).collect()
    }

    /// Replay events to reconstruct exchange state.
    ///
    /// Creates a new exchange and applies all events.
    /// The resulting exchange will be in the exact same state
    /// as one that processed these events originally.
    pub fn replay(events: &[Event]) -> Self {
        let mut exchange = Self::new();
        for event in events {
            exchange.apply(event);
        }
        exchange
    }

    /// Get all recorded events.
    pub fn events(&self) -> &[Event] {
        &self.events
    }

    /// Clear the event log.
    ///
    /// Useful after persisting events to external storage.
    pub fn clear_events(&mut self) {
        self.events.clear();
    }
}

#[cfg(all(test, feature = "event-log"))]
mod tests {
    use super::*;
    use crate::OrderStatus;

    #[test]
    fn event_constructors() {
        let e1 = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        assert!(matches!(e1, Event::SubmitLimit { .. }));

        let e2 = Event::submit_market(Side::Sell, 50);
        assert!(matches!(e2, Event::SubmitMarket { .. }));

        let e3 = Event::cancel(OrderId(1));
        assert!(matches!(e3, Event::Cancel { .. }));

        let e4 = Event::modify(OrderId(1), Price(99_00), 200);
        assert!(matches!(e4, Event::Modify { .. }));
    }

    #[test]
    fn apply_submit_limit() {
        let mut exchange = Exchange::new();

        let event = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.apply(&event);

        assert!(result.trades.is_empty());
        assert_eq!(exchange.best_bid(), Some(Price(100_00)));
        assert_eq!(exchange.events().len(), 1);
    }

    #[test]
    fn apply_submit_with_trade() {
        let mut exchange = Exchange::new();

        // Place resting order directly (not via event for setup)
        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

        // Apply crossing order via event
        let event = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let result = exchange.apply(&event);

        assert_eq!(result.trades.len(), 1);
        assert_eq!(result.trades[0].quantity, 100);
    }

    #[test]
    fn apply_cancel() {
        let mut exchange = Exchange::new();

        let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let event = Event::cancel(submit.order_id);
        let result = exchange.apply(&event);

        assert!(result.trades.is_empty());
        assert_eq!(exchange.best_bid(), None);
    }

    #[test]
    fn apply_modify() {
        let mut exchange = Exchange::new();

        let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

        let event = Event::modify(submit.order_id, Price(99_00), 150);
        let result = exchange.apply(&event);

        assert!(result.trades.is_empty());
        assert_eq!(exchange.best_bid(), Some(Price(99_00)));
    }

    #[test]
    fn apply_all() {
        let mut exchange = Exchange::new();

        let events = vec![
            Event::submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC),
            Event::submit_limit(Side::Buy, Price(100_00), 50, TimeInForce::GTC),
        ];

        let trades = exchange.apply_all(&events);

        assert_eq!(trades.len(), 1);
        assert_eq!(trades[0].quantity, 50);
    }

    #[test]
    fn replay_produces_identical_state() {
        // Create original exchange and perform operations
        let mut original = Exchange::new();

        original.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        original.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        original.submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
        original.submit_limit(Side::Buy, Price(100_00), 75, TimeInForce::GTC); // Crosses

        // Save events
        let events = original.events().to_vec();

        // Replay on fresh exchange
        let replayed = Exchange::replay(&events);

        // Verify identical state
        assert_eq!(original.best_bid_ask(), replayed.best_bid_ask());
        assert_eq!(original.trades().len(), replayed.trades().len());

        // Compare trade details
        for (orig, repl) in original.trades().iter().zip(replayed.trades().iter()) {
            assert_eq!(orig.price, repl.price);
            assert_eq!(orig.quantity, repl.quantity);
            assert_eq!(orig.aggressor_side, repl.aggressor_side);
        }
    }

    #[test]
    fn replay_with_cancels() {
        let mut original = Exchange::new();

        let o1 = original.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let _o2 = original.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
        original.cancel(o1.order_id);

        let events = original.events().to_vec();
        let replayed = Exchange::replay(&events);

        // Only o2 should remain
        assert_eq!(replayed.best_bid(), Some(Price(99_00)));
        assert!(replayed.get_order(o1.order_id).unwrap().status == OrderStatus::Cancelled);
    }

    #[test]
    fn replay_with_modifies() {
        let mut original = Exchange::new();

        let o1 = original.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        original.modify(o1.order_id, Price(101_00), 200);

        let events = original.events().to_vec();
        let replayed = Exchange::replay(&events);

        assert_eq!(replayed.best_bid(), Some(Price(101_00)));
    }

    #[test]
    fn replay_complex_scenario() {
        let mut original = Exchange::new();

        // Build up a book
        original.submit_limit(Side::Sell, Price(102_00), 100, TimeInForce::GTC);
        original.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        original.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
        original.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
        original.submit_limit(Side::Buy, Price(98_00), 100, TimeInForce::GTC);

        // Market buy sweeps some asks
        original.submit_market(Side::Buy, 250);

        // Cancel remaining bid
        let bid = original.submit_limit(Side::Buy, Price(97_00), 50, TimeInForce::GTC);
        original.cancel(bid.order_id);

        let events = original.events().to_vec();
        let replayed = Exchange::replay(&events);

        // Verify snapshots match
        let orig_snap = original.full_book();
        let repl_snap = replayed.full_book();

        assert_eq!(orig_snap.bids.len(), repl_snap.bids.len());
        assert_eq!(orig_snap.asks.len(), repl_snap.asks.len());

        for (o, r) in orig_snap.bids.iter().zip(repl_snap.bids.iter()) {
            assert_eq!(o.price, r.price);
            assert_eq!(o.quantity, r.quantity);
        }
    }

    #[test]
    fn clear_events() {
        let mut exchange = Exchange::new();

        exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        assert_eq!(exchange.events().len(), 1);

        exchange.clear_events();
        assert_eq!(exchange.events().len(), 0);

        // State is preserved
        assert_eq!(exchange.best_bid(), Some(Price(100_00)));
    }

    #[test]
    fn events_are_equal() {
        let e1 = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let e2 = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
        let e3 = Event::submit_limit(Side::Buy, Price(100_00), 200, TimeInForce::GTC);

        assert_eq!(e1, e2);
        assert_ne!(e1, e3);
    }
}
