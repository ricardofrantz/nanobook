//! Reference-parity tests against pinned scipy / TA-Lib / quantstats
//! outputs.
//!
//! The golden fixture at `tests/parity/golden.json` is generated
//! manually by running `tests/parity/generate_golden.py` under the
//! versions pinned in `tests/parity/requirements.txt`. CI only reads
//! the fixture — it does not regenerate.
//!
//! Per-function tolerances are documented per test. Do NOT loosen a
//! tolerance to make a test pass: either the reference convention
//! differs (document it, pick a different reference) or nanobook has a
//! bug to fix.
//!
//! See `tests/parity/README.md` for the full drift policy.
//!
//! This module ships with the v0.10 "Hardening Release" as the
//! measurement substrate for every numerical fix. Per-function
//! reference comparisons live here; pure regression tests for
//! specific bugs (e.g., catastrophic cancellation) live in their own
//! test files alongside the fix that introduces them.
//!
//! Tests in this file:
//!
//! - `rsi_matches_talib`               — initial scaffolding (N10).
//! - `atr_matches_talib`               — initial scaffolding (N10).
//! - `sharpe_matches_quantstats`       — initial scaffolding (N10).
//! - `max_drawdown_matches_quantstats` — initial scaffolding (N10).
//! - `cvar_historical_matches_empirical`  — added by N2 (default method).
//! - `cvar_parametric_matches_quantstats` — added by N2 (legacy method).
//! - `sortino_matches_quantstats`         — added by N4 (ddof=0 default).
//! - `sortino_ddof1_matches_scaled_ddof0` — added by N4 (legacy path).
//!
//! Related regression tests in other files:
//!
//! - `tests/catastrophic_cancellation.rs` — Welford rolling variance
//!   (N1). Separate from this file because it has no scipy/talib/qs
//!   reference; it asserts the output is not collapsed to zero on
//!   pathological input.

use std::path::PathBuf;

use serde_json::Value;

// --- Fixture loader --------------------------------------------------------

fn golden() -> Value {
    let path: PathBuf = [env!("CARGO_MANIFEST_DIR"), "tests", "parity", "golden.json"]
        .iter()
        .collect();
    let raw = std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!(
            "failed to read {}: {e}\n\
             Regenerate with `uv run python tests/parity/generate_golden.py` \
             (see tests/parity/README.md)",
            path.display()
        )
    });
    serde_json::from_str(&raw).expect("golden.json is not valid JSON")
}

/// Extract a `Vec<f64>` from a JSON array of numbers. Panics if the
/// path is missing or contains a non-finite value (use `f64_nullable`
/// for indicator outputs with leading NaN).
fn f64_vec(g: &Value, path: &[&str]) -> Vec<f64> {
    let mut cur = g;
    for key in path {
        cur = cur
            .get(*key)
            .unwrap_or_else(|| panic!("golden.json missing path: {}", path.join(".")));
    }
    cur.as_array()
        .expect("not an array")
        .iter()
        .map(|v| v.as_f64().expect("non-numeric entry"))
        .collect()
}

/// Extract a `Vec<Option<f64>>` from a JSON array where `null`
/// represents NaN. Used for TA-Lib indicator outputs (first `period`
/// entries are `null`).
fn f64_nullable(g: &Value, path: &[&str]) -> Vec<Option<f64>> {
    let mut cur = g;
    for key in path {
        cur = cur
            .get(*key)
            .unwrap_or_else(|| panic!("golden.json missing path: {}", path.join(".")));
    }
    cur.as_array()
        .expect("not an array")
        .iter()
        .map(|v| {
            if v.is_null() {
                None
            } else {
                Some(v.as_f64().expect("non-numeric entry"))
            }
        })
        .collect()
}

fn f64_scalar(g: &Value, path: &[&str]) -> f64 {
    let mut cur = g;
    for key in path {
        cur = cur
            .get(*key)
            .unwrap_or_else(|| panic!("golden.json missing path: {}", path.join(".")));
    }
    cur.as_f64().expect("not a number")
}

// --- Helpers ---------------------------------------------------------------

/// Assert that two `Vec<Option<f64>>` sequences align index-for-index:
/// `None` in the reference must correspond to `NaN` in nanobook's
/// output (and vice versa), and finite values must agree within
/// `tol`.
#[track_caller]
fn assert_indicator_parity(ours: &[f64], theirs: &[Option<f64>], tol: f64, label: &str) {
    assert_eq!(
        ours.len(),
        theirs.len(),
        "{label}: length mismatch ({} vs {})",
        ours.len(),
        theirs.len()
    );
    let mut max_diff = 0.0_f64;
    let mut max_diff_idx = usize::MAX;
    for (i, (o, t)) in ours.iter().zip(theirs.iter()).enumerate() {
        match (o.is_nan(), t) {
            (true, None) => {}
            (false, Some(tv)) => {
                let diff = (o - tv).abs();
                if diff > max_diff {
                    max_diff = diff;
                    max_diff_idx = i;
                }
                assert!(
                    diff <= tol,
                    "{label}[{i}]: ours={o}, reference={tv}, diff={diff} > tol={tol}"
                );
            }
            (true, Some(tv)) => panic!(
                "{label}[{i}]: ours=NaN, reference={tv} (nanobook NaN where reference is finite)"
            ),
            (false, None) => panic!(
                "{label}[{i}]: ours={o}, reference=NaN (nanobook finite where reference is NaN)"
            ),
        }
    }
    eprintln!("{label}: max_diff={max_diff:.3e} at index {max_diff_idx} (tol={tol:.3e})");
}

// --- Scaffolding / integrity tests -----------------------------------------

#[test]
fn golden_fixture_loads() {
    let g = golden();
    // _meta.seed and _meta.n are load-bearing — any regeneration must
    // preserve them.
    assert_eq!(g["_meta"]["seed"].as_i64(), Some(42));
    assert_eq!(g["_meta"]["n"].as_i64(), Some(500));
}

#[test]
fn input_series_have_expected_length() {
    let g = golden();
    for field in ["returns", "close", "highs", "lows"] {
        let v = f64_vec(&g, &["inputs", field]);
        assert_eq!(v.len(), 500, "inputs.{field} wrong length");
    }
}

// --- TA-Lib parity: indicators ---------------------------------------------

/// RSI(14) on the synthetic close series must agree with TA-Lib.
///
/// Tolerance: 1e-6. Nanobook's RSI uses Wilder's smoothing, identical
/// to TA-Lib's `RSI` function.
#[test]
fn rsi_matches_talib() {
    let g = golden();
    let close = f64_vec(&g, &["inputs", "close"]);
    let expected = f64_nullable(&g, &["talib", "rsi_14"]);

    let ours = nanobook::indicators::rsi(&close, 14);
    assert_indicator_parity(&ours, &expected, 1e-6, "rsi_14");
}

/// ATR(14) on the synthetic OHLC series must agree with TA-Lib.
///
/// Tolerance: 1e-6. Nanobook's ATR uses Wilder's smoothing and seeds
/// from `tr[1..=period]`, matching TA-Lib's `ta_ATR.c`.
#[test]
fn atr_matches_talib() {
    let g = golden();
    let highs = f64_vec(&g, &["inputs", "highs"]);
    let lows = f64_vec(&g, &["inputs", "lows"]);
    let close = f64_vec(&g, &["inputs", "close"]);
    let expected = f64_nullable(&g, &["talib", "atr_14"]);

    let ours = nanobook::indicators::atr(&highs, &lows, &close, 14);
    assert_indicator_parity(&ours, &expected, 1e-6, "atr_14");
}

// --- quantstats parity: portfolio metrics ----------------------------------

/// Annualized Sharpe (252 periods/year, rf=0) on the synthetic return
/// series must agree with quantstats.
///
/// Tolerance: 1e-9 — Sharpe is a closed-form ratio of sums, no
/// iteration or smoothing.
#[test]
fn sharpe_matches_quantstats() {
    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let expected = f64_scalar(&g, &["quantstats", "sharpe_annual_252"]);

    let metrics = nanobook::portfolio::metrics::compute_metrics(&returns, 252.0, 0.0)
        .expect("non-empty return series");
    let ours = metrics.sharpe;

    let diff = (ours - expected).abs();
    assert!(
        diff <= 1e-9,
        "sharpe: ours={ours}, quantstats={expected}, diff={diff}"
    );
}

/// Maximum drawdown on the synthetic return series must agree with
/// quantstats up to a sign convention.
///
/// Nanobook returns a positive fraction (0.20 = 20% drawdown);
/// quantstats returns a signed value (-0.20). Compare magnitudes.
///
/// Tolerance: 1e-9.
#[test]
fn max_drawdown_matches_quantstats() {
    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let expected = f64_scalar(&g, &["quantstats", "max_drawdown"]);

    let metrics = nanobook::portfolio::metrics::compute_metrics(&returns, 252.0, 0.0)
        .expect("non-empty return series");
    let ours = metrics.max_drawdown;

    let diff = (ours - expected.abs()).abs();
    assert!(
        diff <= 1e-9,
        "max_drawdown: ours={ours} (positive fraction), \
         quantstats={expected} (signed), |our - |theirs||={diff}"
    );
}

/// Historical CVaR (default in v0.10) must agree with the pure
/// empirical `mean(sorted[..ceil(n * alpha)])` formula at bit-level
/// precision. `compute_metrics.cvar_95` uses this method by default.
///
/// Tolerance: 1e-12 — both sides compute the identical operation
/// (sort, slice, mean).
#[test]
fn cvar_historical_matches_empirical() {
    use nanobook::portfolio::metrics::{CVaRMethod, cvar};

    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let expected = f64_scalar(&g, &["empirical", "cvar_95"]);

    // Direct API.
    let ours_direct = cvar(&returns, 0.05, CVaRMethod::Historical);
    let diff = (ours_direct - expected).abs();
    assert!(
        diff <= 1e-12,
        "cvar(Historical): ours={ours_direct}, empirical={expected}, diff={diff}"
    );

    // The Metrics struct routes through this method too.
    let metrics = nanobook::portfolio::metrics::compute_metrics(&returns, 252.0, 0.0)
        .expect("non-empty return series");
    let diff = (metrics.cvar_95 - expected).abs();
    assert!(
        diff <= 1e-12,
        "metrics.cvar_95 (Historical default): ours={}, empirical={expected}, diff={diff}",
        metrics.cvar_95
    );
}

/// ParametricNormal CVaR (legacy v0.9 behavior) must still agree with
/// quantstats's `expected_shortfall` at 1e-9 — quantstats uses the
/// same hybrid estimator.
///
/// This pins the legacy path so users who opt in via
/// `CVaRMethod::ParametricNormal` continue to get the value they had
/// before v0.10.
#[test]
fn cvar_parametric_matches_quantstats() {
    use nanobook::portfolio::metrics::{CVaRMethod, cvar};

    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let expected = f64_scalar(&g, &["quantstats", "cvar_95_parametric"]);

    let ours = cvar(&returns, 0.05, CVaRMethod::ParametricNormal);
    let diff = (ours - expected).abs();
    assert!(
        diff <= 1e-9,
        "cvar(ParametricNormal): ours={ours}, quantstats={expected}, diff={diff}"
    );
}

/// Annualized Sortino (ddof=0, default in v0.10) must agree with
/// `quantstats.stats.sortino` at 1e-9.
///
/// `compute_metrics.sortino` routes through `sortino(..., ddof=0)` by
/// default. The ddof=1 variant (Bessel-corrected, v0.9 behavior) is
/// not pinned here — callers who need it pass `ddof=1` explicitly and
/// can derive the expected value with `sqrt(n/(n-1))` scaling.
#[test]
fn sortino_matches_quantstats() {
    use nanobook::portfolio::metrics::sortino;

    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let expected = f64_scalar(&g, &["quantstats", "sortino_annual_252"]);

    // Direct API.
    let ours_direct = sortino(&returns, 0.0, 252.0, 0);
    let diff = (ours_direct - expected).abs();
    assert!(
        diff <= 1e-9,
        "sortino(ddof=0) direct: ours={ours_direct}, quantstats={expected}, diff={diff}"
    );

    // The Metrics struct routes through this method too.
    let metrics = nanobook::portfolio::metrics::compute_metrics(&returns, 252.0, 0.0)
        .expect("non-empty return series");
    let diff = (metrics.sortino - expected).abs();
    assert!(
        diff <= 1e-9,
        "metrics.sortino (ddof=0 default): ours={}, quantstats={expected}, diff={diff}",
        metrics.sortino
    );
}

/// Bessel-corrected Sortino (ddof=1, legacy v0.9 behavior) must relate
/// to the ddof=0 result by exactly `sqrt(n/(n-1))`.
///
/// This pins the opt-in legacy path at bit-level.
#[test]
fn sortino_ddof1_matches_scaled_ddof0() {
    use nanobook::portfolio::metrics::sortino;

    let g = golden();
    let returns = f64_vec(&g, &["inputs", "returns"]);
    let n = returns.len() as f64;

    // ddof=1 uses / (n-1), ddof=0 uses / n, so downside_dev ratio is
    // sqrt(n / (n-1)); Sortino is inversely proportional to downside_dev,
    // so ratio is sqrt((n-1)/n).
    let s0 = sortino(&returns, 0.0, 252.0, 0);
    let s1 = sortino(&returns, 0.0, 252.0, 1);
    let ratio = s1 / s0;
    let expected_ratio = ((n - 1.0) / n).sqrt();
    let diff = (ratio - expected_ratio).abs();
    assert!(
        diff <= 1e-12,
        "sortino ddof ratio: got s1/s0={ratio}, expected sqrt((n-1)/n)={expected_ratio}, diff={diff}"
    );
}
