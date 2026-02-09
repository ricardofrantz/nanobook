//! Technical analysis indicators.
//!
//! Drop-in replacements for TA-Lib's RSI, MACD, Bollinger Bands, and ATR.
//! All functions use the same algorithms and conventions as TA-Lib so that
//! outputs are numerically identical (within floating-point tolerance).
//!
//! # Conventions
//!
//! - Input slices are `&[f64]` (closing prices, or OHLC for ATR).
//! - Output `Vec<f64>` has the same length as input; elements within the
//!   lookback period are filled with `f64::NAN`.
//! - **Wilder's smoothing** (RSI, ATR): `alpha = 1/period`, NOT `2/(period+1)`.
//! - **Standard EMA** (MACD): `alpha = 2/(period+1)`.
//!
//! # References
//!
//! - TA-Lib source: `ta_RSI.c`, `ta_MACD.c`, `ta_BBANDS.c`, `ta_ATR.c`
//!   <https://github.com/TA-Lib/ta-lib/tree/main/src/ta_func>

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Standard exponential moving average (alpha = 2/(period+1)).
///
/// Used by MACD (fast EMA, slow EMA, signal line).
fn ema(values: &[f64], period: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if n < period || period == 0 {
        return out;
    }

    // Seed: simple average of first `period` values
    let seed: f64 = values[..period].iter().sum::<f64>() / period as f64;
    out[period - 1] = seed;

    let multiplier = 2.0 / (period as f64 + 1.0);
    for i in period..n {
        out[i] = (values[i] - out[i - 1]) * multiplier + out[i - 1];
    }
    out
}

/// Simple moving average.
fn sma(values: &[f64], period: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if n < period || period == 0 {
        return out;
    }

    let mut window_sum: f64 = values[..period].iter().sum();
    out[period - 1] = window_sum / period as f64;

    for i in period..n {
        window_sum += values[i] - values[i - period];
        out[i] = window_sum / period as f64;
    }
    out
}

/// Population standard deviation over a rolling window.
///
/// Uses O(N) running sum/sum-of-squares instead of O(N*K) re-summation.
/// Returns NaN for the lookback period.
fn rolling_std_pop(values: &[f64], period: usize) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if n < period || period == 0 {
        return out;
    }

    let k = period as f64;

    // Seed: first window
    let mut sum: f64 = values[..period].iter().sum();
    let mut sum_sq: f64 = values[..period].iter().map(|v| v * v).sum();

    let mean = sum / k;
    out[period - 1] = (sum_sq / k - mean * mean).max(0.0).sqrt();

    // Slide window: add new, remove old
    for i in period..n {
        let old = values[i - period];
        let new = values[i];
        sum += new - old;
        sum_sq += new * new - old * old;

        let mean = sum / k;
        out[i] = (sum_sq / k - mean * mean).max(0.0).sqrt();
    }
    out
}

// ---------------------------------------------------------------------------
// Public indicators
// ---------------------------------------------------------------------------

/// Compute RSI value from average gain/loss (TA-Lib convention).
///
/// - Both zero (flat price) returns 0.0.
/// - Zero loss (always up) returns 100.0.
/// - Otherwise: 100 - 100/(1 + gain/loss).
fn rsi_from_avgs(avg_gain: f64, avg_loss: f64) -> f64 {
    if avg_gain == 0.0 && avg_loss == 0.0 {
        0.0
    } else if avg_loss == 0.0 {
        100.0
    } else {
        100.0 - 100.0 / (1.0 + avg_gain / avg_loss)
    }
}

/// Relative Strength Index (Wilder's smoothing).
///
/// Matches TA-Lib `ta_RSI.c` behavior:
/// - Lookback: first `period` elements are NaN.
/// - When all gains are zero (flat price), returns 0.0 (not 50.0).
/// - When all losses are zero (always up), returns 100.0.
///
/// # Arguments
///
/// * `close` — Closing prices.
/// * `period` — Lookback period (typically 14).
///
/// # Example
///
/// ```
/// use nanobook::indicators::rsi;
///
/// let close = vec![44.0, 44.25, 44.50, 43.75, 44.50, 44.25, 43.50,
///                  44.00, 44.50, 43.25, 43.00, 43.50, 44.00, 44.50,
///                  44.25, 44.00, 43.50, 43.75, 44.00, 43.25];
/// let result = rsi(&close, 14);
/// assert!(result[13].is_nan());  // lookback period
/// assert!(!result[14].is_nan()); // first valid RSI
/// ```
pub fn rsi(close: &[f64], period: usize) -> Vec<f64> {
    let n = close.len();
    let mut out = vec![f64::NAN; n];
    if n <= period || period == 0 {
        return out;
    }

    // Seed with simple average over first `period` changes (indices 1..=period)
    let mut avg_gain = 0.0_f64;
    let mut avg_loss = 0.0_f64;
    for i in 1..=period {
        let diff = close[i] - close[i - 1];
        if diff > 0.0 {
            avg_gain += diff;
        } else {
            avg_loss -= diff;
        }
    }
    avg_gain /= period as f64;
    avg_loss /= period as f64;

    // First RSI value
    out[period] = rsi_from_avgs(avg_gain, avg_loss);

    // Subsequent values with Wilder's smoothing
    for i in (period + 1)..n {
        let diff = close[i] - close[i - 1];
        let gain = if diff > 0.0 { diff } else { 0.0 };
        let loss = if diff < 0.0 { -diff } else { 0.0 };
        avg_gain = (avg_gain * (period as f64 - 1.0) + gain) / period as f64;
        avg_loss = (avg_loss * (period as f64 - 1.0) + loss) / period as f64;

        out[i] = rsi_from_avgs(avg_gain, avg_loss);
    }

    out
}

/// Moving Average Convergence Divergence (MACD).
///
/// Matches TA-Lib `ta_MACD.c` behavior:
/// - Fast/slow lines use standard EMA (alpha = 2/(period+1)).
/// - Signal line is EMA of the MACD line.
/// - Histogram = MACD line − signal line.
///
/// Returns `(macd_line, signal_line, histogram)`.
///
/// NaN is filled for the lookback period: `slow_period + signal_period - 2` elements.
///
/// # Arguments
///
/// * `close` — Closing prices.
/// * `fast_period` — Fast EMA period (typically 12).
/// * `slow_period` — Slow EMA period (typically 26).
/// * `signal_period` — Signal line EMA period (typically 9).
pub fn macd(
    close: &[f64],
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = close.len();
    let nan_vec = || vec![f64::NAN; n];

    if n < slow_period
        || fast_period == 0
        || slow_period == 0
        || signal_period == 0
        || fast_period >= slow_period
    {
        return (nan_vec(), nan_vec(), nan_vec());
    }

    // TA-Lib aligns both EMAs so they first produce a value at index slow_period-1.
    // The fast EMA is seeded from close[slow_period-fast_period..slow_period],
    // NOT from close[0..fast_period]. This ensures both EMAs start from the same bar.
    let offset = slow_period - fast_period;
    let fast_ema = ema(&close[offset..], fast_period);
    let slow_ema = ema(close, slow_period);

    // MACD line = fast EMA - slow EMA (valid from slow_period - 1)
    let first_valid = slow_period - 1;
    let mut macd_line = vec![f64::NAN; n];
    for i in first_valid..n {
        let fi = i - offset; // index into fast_ema
        if !fast_ema[fi].is_nan() && !slow_ema[i].is_nan() {
            macd_line[i] = fast_ema[fi] - slow_ema[i];
        }
    }

    // Signal line = EMA of valid MACD values (pass slice directly — no copy)
    let signal_raw = ema(&macd_line[first_valid..], signal_period);

    let mut signal_line = vec![f64::NAN; n];
    for (j, &val) in signal_raw.iter().enumerate() {
        signal_line[first_valid + j] = val;
    }

    // Histogram = MACD - Signal
    let mut histogram = vec![f64::NAN; n];
    for i in 0..n {
        if !macd_line[i].is_nan() && !signal_line[i].is_nan() {
            histogram[i] = macd_line[i] - signal_line[i];
        }
    }

    (macd_line, signal_line, histogram)
}

/// Bollinger Bands (SMA +/- k * population standard deviation).
///
/// Matches TA-Lib `ta_BBANDS.c` behavior:
/// - Middle band = SMA.
/// - Upper band = SMA + num_std_up * stddev.
/// - Lower band = SMA - num_std_dn * stddev.
/// - Uses **population** standard deviation (ddof=0), matching TA-Lib.
///
/// Returns `(upper, middle, lower)`.
///
/// # Arguments
///
/// * `close` — Closing prices.
/// * `period` — SMA/stddev period (typically 20).
/// * `num_std_up` — Number of standard deviations above SMA (typically 2.0).
/// * `num_std_dn` — Number of standard deviations below SMA (typically 2.0).
pub fn bbands(
    close: &[f64],
    period: usize,
    num_std_up: f64,
    num_std_dn: f64,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    let n = close.len();
    let middle = sma(close, period);
    let std = rolling_std_pop(close, period);

    let mut upper = vec![f64::NAN; n];
    let mut lower = vec![f64::NAN; n];

    for i in 0..n {
        if !middle[i].is_nan() {
            upper[i] = middle[i] + num_std_up * std[i];
            lower[i] = middle[i] - num_std_dn * std[i];
        }
    }

    (upper, middle, lower)
}

/// Average True Range (Wilder's smoothing of True Range).
///
/// Matches TA-Lib `ta_ATR.c` behavior:
/// - True Range = max(H-L, |H-C_prev|, |L-C_prev|).
/// - First ATR value = simple average of first `period` True Range values.
/// - Subsequent values use Wilder's smoothing (alpha = 1/period).
///
/// # Arguments
///
/// * `high` — High prices.
/// * `low` — Low prices.
/// * `close` — Closing prices.
/// * `period` — Lookback period (typically 14).
pub fn atr(high: &[f64], low: &[f64], close: &[f64], period: usize) -> Vec<f64> {
    let n = high.len();
    if n != low.len() || n != close.len() {
        return vec![f64::NAN; n];
    }
    if n <= period || period == 0 {
        return vec![f64::NAN; n];
    }

    // Compute True Range series
    let mut tr = vec![0.0_f64; n];
    tr[0] = high[0] - low[0]; // First bar: just H-L (no previous close)
    for i in 1..n {
        let hl = high[i] - low[i];
        let hc = (high[i] - close[i - 1]).abs();
        let lc = (low[i] - close[i - 1]).abs();
        tr[i] = hl.max(hc).max(lc);
    }

    // Apply Wilder's smoothing to True Range (starting from index 1)
    // ATR lookback is `period` bars of True Range (from index 1 onward)
    let mut out = vec![f64::NAN; n];

    // Seed: simple average of first `period` True Range values (starting from index 1)
    let seed: f64 = tr[1..=period].iter().sum::<f64>() / period as f64;
    out[period] = seed;

    // Wilder's recursive smoothing
    for i in (period + 1)..n {
        out[i] = (out[i - 1] * (period as f64 - 1.0) + tr[i]) / period as f64;
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rsi_monotonic_up() {
        let close: Vec<f64> = (1..=100).map(|x| x as f64).collect();
        let result = rsi(&close, 14);
        // All gains, no losses → RSI should be 100
        let last = result.last().unwrap();
        assert!((*last - 100.0).abs() < 1e-10);
    }

    #[test]
    fn rsi_monotonic_down() {
        let close: Vec<f64> = (1..=100).rev().map(|x| x as f64).collect();
        let result = rsi(&close, 14);
        // All losses, no gains → RSI should be 0
        let last = result.last().unwrap();
        assert!(last.abs() < 1e-10);
    }

    #[test]
    fn rsi_constant_price() {
        let close = vec![100.0; 50];
        let result = rsi(&close, 14);
        // Flat price: TA-Lib returns 0.0
        let last = result.last().unwrap();
        assert!(
            last.abs() < 1e-10,
            "expected 0.0 for flat price, got {last}"
        );
    }

    #[test]
    fn rsi_bounds() {
        let close = vec![
            44.0, 44.25, 44.50, 43.75, 44.50, 44.25, 43.50, 44.0, 44.50, 43.25, 43.0, 43.50, 44.0,
            44.50, 44.25, 44.0, 43.50, 43.75, 44.0, 43.25,
        ];
        let result = rsi(&close, 14);
        for (i, &v) in result.iter().enumerate() {
            if !v.is_nan() {
                assert!(
                    (0.0..=100.0).contains(&v),
                    "RSI out of bounds at index {i}: {v}"
                );
            }
        }
    }

    #[test]
    fn rsi_lookback_nan() {
        let close: Vec<f64> = (1..=30).map(|x| x as f64).collect();
        let result = rsi(&close, 14);
        // First 14 elements should be NaN (indices 0..14)
        for (i, v) in result.iter().take(14).enumerate() {
            assert!(v.is_nan(), "expected NaN at index {i}");
        }
        assert!(!result[14].is_nan(), "expected valid RSI at index 14");
    }

    #[test]
    fn macd_basic() {
        let close: Vec<f64> = (1..=50).map(|x| x as f64).collect();
        let (macd_line, signal, histogram) = macd(&close, 12, 26, 9);
        assert_eq!(macd_line.len(), 50);
        assert_eq!(signal.len(), 50);
        assert_eq!(histogram.len(), 50);
        // MACD of uptrend should be positive
        let last_macd = macd_line.last().unwrap();
        assert!(!last_macd.is_nan());
        assert!(*last_macd > 0.0);
    }

    #[test]
    fn bbands_basic() {
        let close: Vec<f64> = (1..=30).map(|x| x as f64).collect();
        let (upper, middle, lower) = bbands(&close, 20, 2.0, 2.0);
        assert_eq!(upper.len(), 30);

        // Check ordering: lower < middle < upper
        for i in 19..30 {
            assert!(
                lower[i] < middle[i] && middle[i] < upper[i],
                "band ordering violated at index {i}"
            );
        }
    }

    #[test]
    fn bbands_constant_price() {
        let close = vec![100.0; 30];
        let (upper, middle, lower) = bbands(&close, 20, 2.0, 2.0);
        // Constant price: std = 0, so upper == middle == lower
        let last = close.len() - 1;
        assert!((upper[last] - 100.0).abs() < 1e-10);
        assert!((middle[last] - 100.0).abs() < 1e-10);
        assert!((lower[last] - 100.0).abs() < 1e-10);
    }

    #[test]
    fn atr_basic() {
        // Simple case: constant range
        let high = vec![102.0; 20];
        let low = vec![98.0; 20];
        let close = vec![100.0; 20];
        let result = atr(&high, &low, &close, 14);

        // True range is always 4.0, so ATR should converge to 4.0
        let last = result.last().unwrap();
        assert!((*last - 4.0).abs() < 0.1, "expected ATR ~4.0, got {last}");
    }

    #[test]
    fn atr_lookback_nan() {
        let high = vec![102.0; 20];
        let low = vec![98.0; 20];
        let close = vec![100.0; 20];
        let result = atr(&high, &low, &close, 14);
        // First 14 elements should be NaN (indices 0..14)
        for (i, v) in result.iter().take(14).enumerate() {
            assert!(v.is_nan(), "expected NaN at index {i}");
        }
        assert!(!result[14].is_nan(), "expected valid ATR at index 14");
    }

    #[test]
    fn empty_input() {
        let empty: Vec<f64> = vec![];
        assert!(rsi(&empty, 14).is_empty());
        let (m, s, h) = macd(&empty, 12, 26, 9);
        assert!(m.is_empty() && s.is_empty() && h.is_empty());
        let (u, mid, l) = bbands(&empty, 20, 2.0, 2.0);
        assert!(u.is_empty() && mid.is_empty() && l.is_empty());
        assert!(atr(&empty, &empty, &empty, 14).is_empty());
    }

    #[test]
    fn insufficient_data() {
        let short = vec![1.0, 2.0, 3.0];
        let result = rsi(&short, 14);
        assert!(result.iter().all(|v| v.is_nan()));
    }
}
