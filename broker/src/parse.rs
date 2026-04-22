//! Shared parse helpers for broker REST responses.
//!
//! Broker APIs deliver numeric fields as decimal strings ("185.50",
//! "0.00012345"). A naive `s.parse::<f64>().unwrap_or(0.0)` silently
//! swallows malformed responses — a garbage balance or price becomes
//! a plausible zero, and downstream P&L and risk accounting never
//! learn the upstream field failed to decode.
//!
//! The helpers here parse with a uniform warn-on-failure policy:
//! every failed parse emits a single `log::warn!` naming the field
//! and the raw string, and the caller gets `0.0` so error-recovery
//! stays graceful. Combined with `types::f64_cents_checked` (S2),
//! the end-to-end chain is:
//!
//! 1. `parse_f64_or_warn(raw, field)` — warn on parse failure,
//!    return `0.0`.
//! 2. `f64_cents_checked(value, field)` — reject NaN/Inf/overflow,
//!    return `Err(BrokerError::NonFiniteValue | ValueOutOfRange)`.
//!
//! The split preserves the "silent zero is OK on junk input, but
//! non-finite and overflow are errors" semantic the broker adapters
//! already rely on.

use log::warn;

/// Parse `raw` as `f64`. On failure, log a `warn!` naming `field`
/// and return `0.0`.
///
/// `field` should be a static, greppable tag like
/// `"binance balance.free"` — this is what surfaces in production
/// logs when an integration partner sends malformed data.
pub(crate) fn parse_f64_or_warn(raw: &str, field: &'static str) -> f64 {
    match raw.parse::<f64>() {
        Ok(v) => v,
        Err(e) => {
            warn!("{field}: failed to parse {raw:?} as f64 ({e}); using 0");
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_decimal() {
        assert_eq!(parse_f64_or_warn("185.50", "test"), 185.50);
    }

    #[test]
    fn parses_zero() {
        assert_eq!(parse_f64_or_warn("0", "test"), 0.0);
        assert_eq!(parse_f64_or_warn("0.0", "test"), 0.0);
    }

    #[test]
    fn parses_negative() {
        assert_eq!(parse_f64_or_warn("-42.99", "test"), -42.99);
    }

    /// Junk → 0.0 (and a warn in the log, but capturing log output is
    /// out of scope for a unit test). The contract is "never panic,
    /// always return a plausible default".
    #[test]
    fn unparseable_returns_zero() {
        assert_eq!(parse_f64_or_warn("not-a-number", "test"), 0.0);
        assert_eq!(parse_f64_or_warn("", "test"), 0.0);
        assert_eq!(parse_f64_or_warn("   ", "test"), 0.0);
    }

    #[test]
    fn handles_scientific_notation() {
        assert_eq!(parse_f64_or_warn("1e3", "test"), 1000.0);
        assert_eq!(parse_f64_or_warn("1.5e-2", "test"), 0.015);
    }

    /// Valid `f64::INFINITY` strings parse successfully; downstream
    /// `f64_cents_checked` is the layer that rejects them.
    #[test]
    fn parses_infinity_strings() {
        assert!(parse_f64_or_warn("inf", "test").is_infinite());
        assert!(parse_f64_or_warn("-inf", "test").is_infinite());
    }
}
