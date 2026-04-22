//! Unit tests for the broker's NaN/overflow-safe f64 → fixed-point
//! integer conversion helpers (S2).
//!
//! The helpers replace the silent saturation pattern of `as i64` on
//! floats:
//! - `NaN as i64` → `0`
//! - `f64::INFINITY as i64` → `i64::MAX`
//! - `f64::NEG_INFINITY as i64` → `i64::MIN`
//! - `(2.0_f64.powi(70)) as i64` → `i64::MAX`
//!
//! These silent paths propagate plausible-looking broker state
//! (positions, balances, prices) downstream; the checked helpers
//! surface them as explicit errors carrying the offending field name.

use nanobook_broker::error::BrokerError;
use nanobook_broker::types::{f64_cents_checked, f64_to_fixed_checked};

// ---------------------------------------------------------------------------
// f64_cents_checked: happy path
// ---------------------------------------------------------------------------

#[test]
fn cents_exact_two_decimal_price() {
    assert_eq!(f64_cents_checked(185.50, "price").unwrap(), 18550);
}

#[test]
fn cents_zero() {
    assert_eq!(f64_cents_checked(0.0, "price").unwrap(), 0);
}

#[test]
fn cents_negative_balance() {
    // Negative cents are legal (margin debits, short positions).
    assert_eq!(f64_cents_checked(-42.99, "cash").unwrap(), -4299);
}

#[test]
fn cents_rounds_half_away_from_zero() {
    // `as i64` truncates toward zero; `f64::round` rounds away from
    // zero. Use a value whose *100 representation has a fractional
    // half: 0.005 * 100 = 0.5, which f64::round → 1.
    let rounded = f64_cents_checked(0.005, "price").unwrap();
    assert_eq!(rounded, 1, "expected half-away rounding, got {rounded}");

    // Same on the negative side: -0.005 → -1, not 0.
    let rounded_neg = f64_cents_checked(-0.005, "price").unwrap();
    assert_eq!(
        rounded_neg, -1,
        "expected symmetric half-away rounding, got {rounded_neg}"
    );
}

#[test]
fn cents_preserves_sub_cent_precision_via_round() {
    // Classic pathology: 111.0 * 0.01 = 1.1100000000000001 in f64.
    // The old truncating path would return 1; the helper rounds to
    // the nearest cent and returns the physically-meaningful 111.
    assert_eq!(f64_cents_checked(1.11, "price").unwrap(), 111);
}

// ---------------------------------------------------------------------------
// f64_cents_checked: NaN / Inf / Out-of-range
// ---------------------------------------------------------------------------

#[test]
fn cents_nan_errors() {
    let err = f64_cents_checked(f64::NAN, "price").unwrap_err();
    assert!(
        matches!(err, BrokerError::NonFiniteValue { field: "price", .. }),
        "got: {err:?}"
    );
}

#[test]
fn cents_positive_inf_errors() {
    let err = f64_cents_checked(f64::INFINITY, "equity").unwrap_err();
    assert!(
        matches!(
            err,
            BrokerError::NonFiniteValue {
                field: "equity",
                ..
            }
        ),
        "got: {err:?}"
    );
}

#[test]
fn cents_negative_inf_errors() {
    let err = f64_cents_checked(f64::NEG_INFINITY, "pnl").unwrap_err();
    assert!(
        matches!(err, BrokerError::NonFiniteValue { field: "pnl", .. }),
        "got: {err:?}"
    );
}

#[test]
fn cents_overflow_positive_errors() {
    // A value whose scaled magnitude exceeds i64::MAX ≈ 9.22e18.
    // 1e20 * 100 = 1e22 — well over the bound.
    let err = f64_cents_checked(1e20, "equity").unwrap_err();
    assert!(
        matches!(
            err,
            BrokerError::ValueOutOfRange {
                field: "equity",
                ..
            }
        ),
        "got: {err:?}"
    );
}

#[test]
fn cents_overflow_negative_errors() {
    let err = f64_cents_checked(-1e20, "equity").unwrap_err();
    assert!(
        matches!(
            err,
            BrokerError::ValueOutOfRange {
                field: "equity",
                ..
            }
        ),
        "got: {err:?}"
    );
}

#[test]
fn cents_near_i64_max_ok() {
    // Largest dollar amount whose *100 still fits. 2^61 dollars
    // = ~2.3e18, scaled to 2.3e20 cents — but 2.3e20 > i64::MAX.
    // Use a safer bound: 5e16 dollars → 5e18 cents, fits.
    let v = 5e16;
    let cents = f64_cents_checked(v, "equity").unwrap();
    assert!(cents > 0);
}

// ---------------------------------------------------------------------------
// f64_to_fixed_checked: arbitrary scale (satoshi example)
// ---------------------------------------------------------------------------

#[test]
fn fixed_satoshis_round_trip() {
    // 1.5 BTC → 150_000_000 satoshis.
    let sats = f64_to_fixed_checked(1.5, 1e8, "btc").unwrap();
    assert_eq!(sats, 150_000_000);
}

#[test]
fn fixed_satoshis_nan_errors() {
    let err = f64_to_fixed_checked(f64::NAN, 1e8, "btc").unwrap_err();
    assert!(matches!(
        err,
        BrokerError::NonFiniteValue { field: "btc", .. }
    ));
}

#[test]
fn fixed_satoshis_overflow_errors() {
    // A balance so large that even at 1e8 satoshis/BTC it doesn't fit:
    // 1e12 BTC * 1e8 = 1e20 sats > i64::MAX.
    let err = f64_to_fixed_checked(1e12, 1e8, "btc").unwrap_err();
    assert!(matches!(
        err,
        BrokerError::ValueOutOfRange { field: "btc", .. }
    ));
}

#[test]
fn fixed_scale_one_is_identity_with_rounding() {
    assert_eq!(f64_to_fixed_checked(42.4, 1.0, "x").unwrap(), 42);
    assert_eq!(f64_to_fixed_checked(42.6, 1.0, "x").unwrap(), 43);
    assert_eq!(f64_to_fixed_checked(-42.6, 1.0, "x").unwrap(), -43);
}

// ---------------------------------------------------------------------------
// Compares silent `as i64` vs. checked path to document the pathology.
// ---------------------------------------------------------------------------

#[test]
#[allow(clippy::cast_nan_to_int)] // Documenting the pathology we replace.
fn documents_silent_saturation_that_helper_replaces() {
    // These are the broken legacy behaviors — pinned here so the
    // contrast with the checked helper is explicit.
    assert_eq!(f64::NAN as i64, 0);
    assert_eq!(f64::INFINITY as i64, i64::MAX);
    assert_eq!(f64::NEG_INFINITY as i64, i64::MIN);

    // The checked helper rejects all three.
    assert!(f64_cents_checked(f64::NAN, "x").is_err());
    assert!(f64_cents_checked(f64::INFINITY, "x").is_err());
    assert!(f64_cents_checked(f64::NEG_INFINITY, "x").is_err());
}
