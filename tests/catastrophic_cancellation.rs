//! Regression tests for catastrophic cancellation in rolling variance.
//!
//! Before v0.10 (item N1), rolling variance was maintained with an
//! O(1) sliding-sum state:
//!
//! ```ignore
//! sum    += new - old;
//! sum_sq += new * new - old * old;
//! variance = sum_sq / k - (sum / k).powi(2);  // or the ddof=1 variant
//! ```
//!
//! For high-mean, low-variance series (e.g., a $1000 stock with
//! sub-cent moves), both `sum_sq` and `(sum/k)^2` are large nearly-equal
//! numbers whose subtraction loses all precision to rounding. The final
//! `.max(0.0)` guard then silently clamped the cancelled variance to
//! zero, so:
//!
//! - `rolling_std_pop` returned exactly 0, and Bollinger bands collapsed
//!   to the middle band.
//! - `rolling_sharpe` divided by zero and returned 0 (via explicit
//!   guard) or NaN.
//! - `rolling_volatility` returned 0.
//!
//! None of this surfaced in existing tests, which used small numbers
//! (returns on the order of 0.01) where the cancellation is mild.
//!
//! The fix in N1 replaced the sliding state with per-window Welford
//! recompute. The tests below exercise the exact bug and must PASS
//! post-fix, FAIL pre-fix.

use nanobook::indicators::bbands;
use nanobook::portfolio::metrics::{rolling_sharpe, rolling_volatility};

/// `[1000.0 + 1e-9 * i for i in 0..100]` — a monotone series with
/// mean ≈ 1000 and a tiny but strictly non-zero standard deviation
/// (~2.89e-8 over the full series, ~5.77e-9 over a 20-element window).
///
/// Under the old sum-of-squares formula, this returned variance ≈ 0
/// via rounding. Under Welford, the standard deviation is correctly
/// non-zero.
fn high_mean_low_variance_series() -> Vec<f64> {
    (0..100).map(|i| 1000.0 + 1e-9 * i as f64).collect()
}

/// `bbands` is the public surface for Bollinger bands and uses
/// `rolling_std_pop` internally. A zero std collapses upper == middle
/// == lower.
#[test]
fn bollinger_bands_do_not_collapse_on_high_mean_low_variance() {
    let close = high_mean_low_variance_series();
    let (upper, middle, lower) = bbands(&close, 20, 2.0, 2.0);

    // First `period - 1 = 19` values are NaN (unchanged).
    for i in 0..19 {
        assert!(upper[i].is_nan(), "bband upper[{i}]: expected NaN");
        assert!(middle[i].is_nan(), "bband middle[{i}]: expected NaN");
        assert!(lower[i].is_nan(), "bband lower[{i}]: expected NaN");
    }

    // From index 19 onward the bands must NOT be collapsed. Index-based
    // loop because the assertion references three parallel arrays.
    #[allow(clippy::needless_range_loop)]
    for i in 19..close.len() {
        let width = upper[i] - lower[i];
        assert!(
            width > 0.0,
            "bband collapse at index {i}: \
             upper={}, middle={}, lower={}, width={}",
            upper[i],
            middle[i],
            lower[i],
            width
        );
        assert!(
            upper[i] > middle[i],
            "band ordering violated at {i}: upper={}, middle={}",
            upper[i],
            middle[i]
        );
        assert!(
            lower[i] < middle[i],
            "band ordering violated at {i}: lower={}, middle={}",
            lower[i],
            middle[i]
        );
    }
}

/// `rolling_volatility` over the same series must return a
/// non-zero annualized volatility.
#[test]
fn rolling_volatility_nonzero_on_high_mean_low_variance() {
    let values = high_mean_low_variance_series();
    let vol = rolling_volatility(&values, 20, 252);

    for (i, v) in vol.iter().enumerate().skip(19) {
        assert!(
            *v > 0.0,
            "rolling_volatility[{i}] = {v} (expected strictly positive)"
        );
        assert!(
            v.is_finite(),
            "rolling_volatility[{i}] = {v} (expected finite)"
        );
    }
}

/// `rolling_sharpe` on a perturbed positive-mean series must produce a
/// finite, non-zero Sharpe. Under the old formula, std collapsed and
/// the ratio reported 0 (via explicit zero-std guard).
#[test]
fn rolling_sharpe_nonzero_on_perturbed_positive_mean_series() {
    // Mean return 10 bps per period, tiny perturbation.
    let returns: Vec<f64> = (0..100).map(|i| 0.001 + 1e-12 * i as f64).collect();
    let sharpe = rolling_sharpe(&returns, 20, 252);

    for (i, s) in sharpe.iter().enumerate().skip(19) {
        assert!(s.is_finite(), "rolling_sharpe[{i}] = {s} (expected finite)");
        assert!(
            *s > 0.0,
            "rolling_sharpe[{i}] = {s} (expected > 0 on positive-mean series)"
        );
    }
}

/// Sanity check: Welford must still match the naive formula for
/// well-conditioned series (returns on the order of 0.01). This is
/// what existing tests exercise; N1 must not regress it.
#[test]
fn welford_matches_naive_on_well_conditioned_series() {
    // Tiny symmetric perturbation around zero — both formulas are
    // numerically fine here.
    let returns: Vec<f64> = (0..50)
        .map(|i| if i % 2 == 0 { 0.01 } else { -0.01 })
        .collect();
    let vol = rolling_volatility(&returns, 10, 252);

    // Alternating ±0.01: sample variance ≈ 0.01^2 * 10/9 for window=10.
    // Population variance = 0.01^2 = 1e-4. Sample = 1.111e-4.
    // Annualized sample std = sqrt(1.111e-4) * sqrt(252) ≈ 0.1673.
    let expected_annualized_std = (1.0e-4_f64 * 10.0 / 9.0).sqrt() * 252.0_f64.sqrt();
    for (i, v) in vol.iter().enumerate().skip(9) {
        assert!(
            (v - expected_annualized_std).abs() < 1e-10,
            "rolling_volatility[{i}] = {v}, expected ≈ {expected_annualized_std} \
             (well-conditioned check)"
        );
    }
}
