//! Order side: Buy or Sell

use std::fmt;

/// Side of an order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    /// Returns the opposite side.
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

impl fmt::Display for Side {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Side::Buy => write!(f, "BUY"),
            Side::Sell => write!(f, "SELL"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opposite() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);
    }

    #[test]
    fn opposite_is_involution() {
        assert_eq!(Side::Buy.opposite().opposite(), Side::Buy);
        assert_eq!(Side::Sell.opposite().opposite(), Side::Sell);
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", Side::Buy), "BUY");
        assert_eq!(format!("{}", Side::Sell), "SELL");
    }
}
