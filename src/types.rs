//! Core types: Price, Quantity, Timestamp, OrderId, TradeId

use std::fmt;

/// Price in smallest units (e.g., cents, basis points).
///
/// `Price(10050)` represents $100.50 if tick size is $0.01.
/// Using fixed-point avoids floating-point errors in financial calculations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(pub i64);

impl Price {
    pub const ZERO: Price = Price(0);
    pub const MAX: Price = Price(i64::MAX);
    pub const MIN: Price = Price(i64::MIN);
}

impl fmt::Display for Price {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Display as dollars.cents assuming cents
        let dollars = self.0 / 100;
        let cents = (self.0 % 100).abs();
        if self.0 < 0 {
            write!(f, "-${}.{:02}", dollars.abs(), cents)
        } else {
            write!(f, "${}.{:02}", dollars, cents)
        }
    }
}

/// Quantity of shares/contracts. Always positive.
pub type Quantity = u64;

/// Timestamp in nanoseconds since exchange start.
/// Monotonically increasing, assigned by exchange.
pub type Timestamp = u64;

/// Unique order identifier assigned by exchange.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "O{}", self.0)
    }
}

/// Unique trade identifier assigned by exchange.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct TradeId(pub u64);

impl fmt::Display for TradeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn price_ordering() {
        assert!(Price(100) < Price(200));
        assert!(Price(-50) < Price(50));
        assert_eq!(Price(100), Price(100));
    }

    #[test]
    fn price_display() {
        assert_eq!(format!("{}", Price(10050)), "$100.50");
        assert_eq!(format!("{}", Price(100)), "$1.00");
        assert_eq!(format!("{}", Price(5)), "$0.05");
        assert_eq!(format!("{}", Price(-250)), "-$2.50");
    }

    #[test]
    fn order_id_display() {
        assert_eq!(format!("{}", OrderId(42)), "O42");
    }

    #[test]
    fn trade_id_display() {
        assert_eq!(format!("{}", TradeId(7)), "T7");
    }
}
