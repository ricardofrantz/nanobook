//! Core types: Price, Quantity, Timestamp, OrderId, TradeId, Symbol

use std::fmt;

/// Price in smallest units (e.g., cents, basis points).
///
/// `Price(10050)` represents $100.50 if tick size is $0.01.
/// Using fixed-point avoids floating-point errors in financial calculations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct OrderId(pub u64);

impl fmt::Display for OrderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "O{}", self.0)
    }
}

/// Unique trade identifier assigned by exchange.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TradeId(pub u64);

impl fmt::Display for TradeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "T{}", self.0)
    }
}

/// A fixed-size symbol identifier (e.g., "AAPL", "MSFT").
///
/// Stored inline as `[u8; 8]` with a length byte — no heap allocation, `Copy`,
/// and suitable for use as a hash map key. Maximum 8 ASCII bytes.
///
/// ```
/// use nanobook::Symbol;
///
/// let sym = Symbol::new("AAPL");
/// assert_eq!(sym.as_str(), "AAPL");
/// assert_eq!(format!("{sym}"), "AAPL");
/// ```
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Symbol {
    buf: [u8; 8],
    len: u8,
}

impl Symbol {
    /// Create a symbol from a string slice. Panics if longer than 8 bytes.
    pub fn new(s: &str) -> Self {
        Self::try_new(s).expect("Symbol must be at most 8 bytes")
    }

    /// Try to create a symbol. Returns `None` if longer than 8 bytes.
    pub fn try_new(s: &str) -> Option<Self> {
        if s.len() > 8 {
            return None;
        }
        let mut buf = [0u8; 8];
        buf[..s.len()].copy_from_slice(s.as_bytes());
        Some(Self {
            buf,
            len: s.len() as u8,
        })
    }

    /// Returns the symbol as a string slice.
    #[inline]
    pub fn as_str(&self) -> &str {
        // Safety: we only accept valid str input in constructors
        unsafe { std::str::from_utf8_unchecked(&self.buf[..self.len as usize]) }
    }
}

impl AsRef<str> for Symbol {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Symbol(\"{}\")", self.as_str())
    }
}

#[cfg(feature = "serde")]
impl serde::Serialize for Symbol {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for Symbol {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let s = <&str>::deserialize(deserializer)?;
        Symbol::try_new(s).ok_or_else(|| serde::de::Error::custom("Symbol must be at most 8 bytes"))
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

    // === Symbol tests ===

    #[test]
    fn symbol_new() {
        let sym = Symbol::new("AAPL");
        assert_eq!(sym.as_str(), "AAPL");
    }

    #[test]
    fn symbol_display() {
        assert_eq!(format!("{}", Symbol::new("MSFT")), "MSFT");
    }

    #[test]
    fn symbol_debug() {
        assert_eq!(format!("{:?}", Symbol::new("GOOG")), "Symbol(\"GOOG\")");
    }

    #[test]
    fn symbol_max_length() {
        let sym = Symbol::new("12345678");
        assert_eq!(sym.as_str(), "12345678");
    }

    #[test]
    fn symbol_try_new_too_long() {
        assert!(Symbol::try_new("123456789").is_none());
    }

    #[test]
    fn symbol_empty() {
        let sym = Symbol::new("");
        assert_eq!(sym.as_str(), "");
    }

    #[test]
    fn symbol_ordering() {
        assert!(Symbol::new("AAPL") < Symbol::new("MSFT"));
        assert_eq!(Symbol::new("AAPL"), Symbol::new("AAPL"));
    }

    #[test]
    fn symbol_hash_eq() {
        use std::collections::HashMap;
        let mut map = HashMap::new();
        map.insert(Symbol::new("AAPL"), 42);
        assert_eq!(map[&Symbol::new("AAPL")], 42);
    }

    #[test]
    fn symbol_copy() {
        let a = Symbol::new("AAPL");
        let b = a; // Copy
        assert_eq!(a, b);
    }

    #[test]
    #[should_panic(expected = "at most 8 bytes")]
    fn symbol_new_panics_too_long() {
        Symbol::new("TOOLONGNAME");
    }
}
