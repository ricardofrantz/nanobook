use nanobook::indicators;
use pyo3::prelude::*;

/// Compute RSI (Relative Strength Index) using Wilder's smoothing.
///
/// Drop-in replacement for ``talib.RSI(close, timeperiod)``.
///
/// Args:
///     close: List of closing prices.
///     period: Lookback period (default 14).
///
/// Returns:
///     List of RSI values. NaN for the lookback period.
///
/// Example::
///
///     rsi = nanobook.py_rsi([44.0, 44.25, 44.5, ...], 14)
///
#[pyfunction]
#[pyo3(signature = (close, period=14))]
pub fn py_rsi(close: Vec<f64>, period: usize) -> Vec<f64> {
    indicators::rsi(&close, period)
}

/// Compute MACD (Moving Average Convergence Divergence).
///
/// Drop-in replacement for ``talib.MACD(close, fast, slow, signal)``.
///
/// Args:
///     close: List of closing prices.
///     fast_period: Fast EMA period (default 12).
///     slow_period: Slow EMA period (default 26).
///     signal_period: Signal line EMA period (default 9).
///
/// Returns:
///     Tuple of (macd_line, signal_line, histogram).
///
/// Example::
///
///     macd, signal, hist = nanobook.py_macd(closes, 12, 26, 9)
///
#[pyfunction]
#[pyo3(signature = (close, fast_period=12, slow_period=26, signal_period=9))]
pub fn py_macd(
    close: Vec<f64>,
    fast_period: usize,
    slow_period: usize,
    signal_period: usize,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    indicators::macd(&close, fast_period, slow_period, signal_period)
}

/// Compute Bollinger Bands (SMA +/- k * standard deviation).
///
/// Drop-in replacement for ``talib.BBANDS(close, period, nbdevup, nbdevdn)``.
///
/// Args:
///     close: List of closing prices.
///     period: SMA/stddev period (default 20).
///     num_std_up: Standard deviations above SMA (default 2.0).
///     num_std_dn: Standard deviations below SMA (default 2.0).
///
/// Returns:
///     Tuple of (upper_band, middle_band, lower_band).
///
/// Example::
///
///     upper, middle, lower = nanobook.py_bbands(closes, 20, 2.0, 2.0)
///
#[pyfunction]
#[pyo3(signature = (close, period=20, num_std_up=2.0, num_std_dn=2.0))]
pub fn py_bbands(
    close: Vec<f64>,
    period: usize,
    num_std_up: f64,
    num_std_dn: f64,
) -> (Vec<f64>, Vec<f64>, Vec<f64>) {
    indicators::bbands(&close, period, num_std_up, num_std_dn)
}

/// Compute ATR (Average True Range) using Wilder's smoothing.
///
/// Drop-in replacement for ``talib.ATR(high, low, close, timeperiod)``.
///
/// Args:
///     high: List of high prices.
///     low: List of low prices.
///     close: List of closing prices.
///     period: Lookback period (default 14).
///
/// Returns:
///     List of ATR values. NaN for the lookback period.
///
/// Example::
///
///     atr = nanobook.py_atr(highs, lows, closes, 14)
///
#[pyfunction]
#[pyo3(signature = (high, low, close, period=14))]
pub fn py_atr(high: Vec<f64>, low: Vec<f64>, close: Vec<f64>, period: usize) -> Vec<f64> {
    indicators::atr(&high, &low, &close, period)
}
