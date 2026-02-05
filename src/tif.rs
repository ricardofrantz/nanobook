//! Time-in-force: controls order lifetime and partial fill behavior

use std::fmt;

/// Time-in-force determines how long an order remains active
/// and how partial fills are handled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimeInForce {
    /// Good-til-cancelled: rests on book until filled or explicitly cancelled.
    /// Allows partial fills; remainder stays on book.
    #[default]
    GTC,

    /// Immediate-or-cancel: fill what's available immediately, cancel remainder.
    /// Allows partial fills; remainder is cancelled (never rests).
    IOC,

    /// Fill-or-kill: fill entire quantity immediately or cancel entire order.
    /// No partial fills allowed.
    FOK,
}

impl TimeInForce {
    /// Returns true if this TIF allows the order to rest on the book.
    #[inline]
    pub fn can_rest(self) -> bool {
        matches!(self, TimeInForce::GTC)
    }

    /// Returns true if this TIF allows partial fills.
    #[inline]
    pub fn allows_partial(self) -> bool {
        matches!(self, TimeInForce::GTC | TimeInForce::IOC)
    }
}

impl fmt::Display for TimeInForce {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimeInForce::GTC => write!(f, "GTC"),
            TimeInForce::IOC => write!(f, "IOC"),
            TimeInForce::FOK => write!(f, "FOK"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_gtc() {
        assert_eq!(TimeInForce::default(), TimeInForce::GTC);
    }

    #[test]
    fn can_rest() {
        assert!(TimeInForce::GTC.can_rest());
        assert!(!TimeInForce::IOC.can_rest());
        assert!(!TimeInForce::FOK.can_rest());
    }

    #[test]
    fn allows_partial() {
        assert!(TimeInForce::GTC.allows_partial());
        assert!(TimeInForce::IOC.allows_partial());
        assert!(!TimeInForce::FOK.allows_partial());
    }

    #[test]
    fn display() {
        assert_eq!(format!("{}", TimeInForce::GTC), "GTC");
        assert_eq!(format!("{}", TimeInForce::IOC), "IOC");
        assert_eq!(format!("{}", TimeInForce::FOK), "FOK");
    }
}
