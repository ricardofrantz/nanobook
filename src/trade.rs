//! Trade representation

use crate::{OrderId, Price, Quantity, Side, Timestamp, TradeId};
use std::fmt;

/// A completed trade between two orders.
///
/// Trades are created when an incoming (aggressor) order matches
/// against a resting (passive) order on the book.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Trade {
    /// Unique identifier assigned by exchange
    pub id: TradeId,
    /// Execution price (always the resting order's price)
    pub price: Price,
    /// Quantity executed
    pub quantity: Quantity,
    /// Order that initiated the trade (taker)
    pub aggressor_order_id: OrderId,
    /// Order that was resting on the book (maker)
    pub passive_order_id: OrderId,
    /// Side of the aggressor order
    pub aggressor_side: Side,
    /// When the trade occurred
    pub timestamp: Timestamp,
}

impl Trade {
    /// Create a new trade.
    pub fn new(
        id: TradeId,
        price: Price,
        quantity: Quantity,
        aggressor_order_id: OrderId,
        passive_order_id: OrderId,
        aggressor_side: Side,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            id,
            price,
            quantity,
            aggressor_order_id,
            passive_order_id,
            aggressor_side,
            timestamp,
        }
    }

    /// Returns the side of the passive (maker) order.
    #[inline]
    pub fn passive_side(&self) -> Side {
        self.aggressor_side.opposite()
    }

    /// Returns the notional value (price × quantity).
    ///
    /// Note: This returns the raw product of price units × quantity.
    /// Interpretation depends on your price unit convention.
    #[inline]
    pub fn notional(&self) -> i64 {
        self.price.0 * self.quantity as i64
    }
}

impl fmt::Display for Trade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} {} @ {} ({} aggressor)",
            self.id,
            self.quantity,
            if self.aggressor_side == Side::Buy {
                "bought"
            } else {
                "sold"
            },
            self.price,
            self.aggressor_order_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trade() -> Trade {
        Trade::new(
            TradeId(1),
            Price(100_50), // $100.50
            100,
            OrderId(10), // aggressor
            OrderId(5),  // passive
            Side::Buy,
            1000,
        )
    }

    #[test]
    fn trade_creation() {
        let trade = make_trade();

        assert_eq!(trade.id, TradeId(1));
        assert_eq!(trade.price, Price(100_50));
        assert_eq!(trade.quantity, 100);
        assert_eq!(trade.aggressor_order_id, OrderId(10));
        assert_eq!(trade.passive_order_id, OrderId(5));
        assert_eq!(trade.aggressor_side, Side::Buy);
        assert_eq!(trade.timestamp, 1000);
    }

    #[test]
    fn passive_side() {
        let buy_aggressor = make_trade();
        assert_eq!(buy_aggressor.passive_side(), Side::Sell);

        let sell_aggressor = Trade::new(
            TradeId(2),
            Price(99_00),
            50,
            OrderId(11),
            OrderId(6),
            Side::Sell,
            2000,
        );
        assert_eq!(sell_aggressor.passive_side(), Side::Buy);
    }

    #[test]
    fn notional_value() {
        let trade = make_trade();
        // 10050 (cents) * 100 (shares) = 1_005_000 cent-shares
        // Interpretation: $10,050.00 notional value
        assert_eq!(trade.notional(), 1_005_000);
    }

    #[test]
    fn display() {
        let trade = make_trade();
        let s = format!("{}", trade);
        assert!(s.contains("T1"));
        assert!(s.contains("100"));
        assert!(s.contains("bought"));
        assert!(s.contains("$100.50"));
        assert!(s.contains("O10"));
    }
}
