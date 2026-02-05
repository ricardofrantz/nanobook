//! Order representation and lifecycle

use crate::{OrderId, Price, Quantity, Side, TimeInForce, Timestamp};

/// Status of an order in its lifecycle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OrderStatus {
    /// Order accepted, resting on book (no fills yet)
    #[default]
    New,
    /// Some quantity filled, remainder still on book
    PartiallyFilled,
    /// Fully executed, no longer on book
    Filled,
    /// Removed by user request or TIF rules, no longer on book
    Cancelled,
}

impl OrderStatus {
    /// Returns true if the order is still active (can be filled or cancelled).
    #[inline]
    pub fn is_active(self) -> bool {
        matches!(self, OrderStatus::New | OrderStatus::PartiallyFilled)
    }

    /// Returns true if the order is terminal (no further state changes).
    #[inline]
    pub fn is_terminal(self) -> bool {
        matches!(self, OrderStatus::Filled | OrderStatus::Cancelled)
    }
}

/// An order in the order book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Order {
    /// Unique identifier assigned by exchange
    pub id: OrderId,
    /// Buy or sell
    pub side: Side,
    /// Limit price (max for buy, min for sell)
    pub price: Price,
    /// Original quantity when submitted
    pub original_quantity: Quantity,
    /// Quantity still available to fill
    pub remaining_quantity: Quantity,
    /// Quantity that has been filled
    pub filled_quantity: Quantity,
    /// When the order was received by exchange
    pub timestamp: Timestamp,
    /// How long the order stays active
    pub time_in_force: TimeInForce,
    /// Current lifecycle status
    pub status: OrderStatus,
}

impl Order {
    /// Create a new order with the given parameters.
    ///
    /// The order starts with `remaining_quantity == original_quantity`,
    /// `filled_quantity == 0`, and `status == New`.
    pub fn new(
        id: OrderId,
        side: Side,
        price: Price,
        quantity: Quantity,
        timestamp: Timestamp,
        time_in_force: TimeInForce,
    ) -> Self {
        Self {
            id,
            side,
            price,
            original_quantity: quantity,
            remaining_quantity: quantity,
            filled_quantity: 0,
            timestamp,
            time_in_force,
            status: OrderStatus::New,
        }
    }

    /// Returns true if the order can still be filled or cancelled.
    #[inline]
    pub fn is_active(&self) -> bool {
        self.status.is_active()
    }

    /// Fill the order by the given quantity.
    ///
    /// Updates `remaining_quantity`, `filled_quantity`, and `status`.
    ///
    /// # Panics
    ///
    /// Panics if `quantity > remaining_quantity`.
    pub fn fill(&mut self, quantity: Quantity) {
        assert!(
            quantity <= self.remaining_quantity,
            "fill quantity {} exceeds remaining {}",
            quantity,
            self.remaining_quantity
        );

        self.remaining_quantity -= quantity;
        self.filled_quantity += quantity;

        self.status = if self.remaining_quantity == 0 {
            OrderStatus::Filled
        } else {
            OrderStatus::PartiallyFilled
        };
    }

    /// Cancel the order, setting status to Cancelled.
    ///
    /// Returns the quantity that was cancelled (remaining at time of cancel).
    ///
    /// # Panics
    ///
    /// Panics if the order is already in a terminal state.
    pub fn cancel(&mut self) -> Quantity {
        assert!(
            self.is_active(),
            "cannot cancel order in terminal state {:?}",
            self.status
        );

        let cancelled = self.remaining_quantity;
        self.remaining_quantity = 0;
        self.status = OrderStatus::Cancelled;
        cancelled
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_order(quantity: Quantity) -> Order {
        Order::new(
            OrderId(1),
            Side::Buy,
            Price(100_00),
            quantity,
            1,
            TimeInForce::GTC,
        )
    }

    #[test]
    fn new_order_initial_state() {
        let order = make_order(100);

        assert_eq!(order.original_quantity, 100);
        assert_eq!(order.remaining_quantity, 100);
        assert_eq!(order.filled_quantity, 0);
        assert_eq!(order.status, OrderStatus::New);
        assert!(order.is_active());
    }

    #[test]
    fn partial_fill() {
        let mut order = make_order(100);

        order.fill(30);

        assert_eq!(order.remaining_quantity, 70);
        assert_eq!(order.filled_quantity, 30);
        assert_eq!(order.status, OrderStatus::PartiallyFilled);
        assert!(order.is_active());
    }

    #[test]
    fn full_fill() {
        let mut order = make_order(100);

        order.fill(100);

        assert_eq!(order.remaining_quantity, 0);
        assert_eq!(order.filled_quantity, 100);
        assert_eq!(order.status, OrderStatus::Filled);
        assert!(!order.is_active());
    }

    #[test]
    fn multiple_partial_fills() {
        let mut order = make_order(100);

        order.fill(30);
        order.fill(50);
        order.fill(20);

        assert_eq!(order.remaining_quantity, 0);
        assert_eq!(order.filled_quantity, 100);
        assert_eq!(order.status, OrderStatus::Filled);
    }

    #[test]
    #[should_panic(expected = "fill quantity 101 exceeds remaining 100")]
    fn fill_exceeds_remaining_panics() {
        let mut order = make_order(100);
        order.fill(101);
    }

    #[test]
    fn cancel_new_order() {
        let mut order = make_order(100);

        let cancelled = order.cancel();

        assert_eq!(cancelled, 100);
        assert_eq!(order.remaining_quantity, 0);
        assert_eq!(order.status, OrderStatus::Cancelled);
        assert!(!order.is_active());
    }

    #[test]
    fn cancel_partially_filled_order() {
        let mut order = make_order(100);
        order.fill(30);

        let cancelled = order.cancel();

        assert_eq!(cancelled, 70);
        assert_eq!(order.filled_quantity, 30);
        assert_eq!(order.remaining_quantity, 0);
        assert_eq!(order.status, OrderStatus::Cancelled);
    }

    #[test]
    #[should_panic(expected = "cannot cancel order in terminal state")]
    fn cancel_filled_order_panics() {
        let mut order = make_order(100);
        order.fill(100);
        order.cancel();
    }

    #[test]
    #[should_panic(expected = "cannot cancel order in terminal state")]
    fn cancel_already_cancelled_panics() {
        let mut order = make_order(100);
        order.cancel();
        order.cancel();
    }

    #[test]
    fn order_status_is_active() {
        assert!(OrderStatus::New.is_active());
        assert!(OrderStatus::PartiallyFilled.is_active());
        assert!(!OrderStatus::Filled.is_active());
        assert!(!OrderStatus::Cancelled.is_active());
    }

    #[test]
    fn order_status_is_terminal() {
        assert!(!OrderStatus::New.is_terminal());
        assert!(!OrderStatus::PartiallyFilled.is_terminal());
        assert!(OrderStatus::Filled.is_terminal());
        assert!(OrderStatus::Cancelled.is_terminal());
    }

    #[test]
    fn quantity_invariant_holds() {
        let mut order = make_order(100);

        // After partial fill
        order.fill(30);
        assert_eq!(
            order.original_quantity,
            order.remaining_quantity + order.filled_quantity
        );

        // After another fill
        order.fill(50);
        assert_eq!(
            order.original_quantity,
            order.remaining_quantity + order.filled_quantity
        );
    }
}
