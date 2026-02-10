use nanobook::garch;
use pyo3::prelude::*;

/// One-step-ahead GARCH-style volatility forecast.
///
/// Args:
///     returns: Return series as decimal fractions.
///     p: ARCH lag count (default 1).
///     q: GARCH lag count (default 1).
///     mean: Mean model, ``"zero"`` or ``"constant"`` (default ``"zero"``).
///
/// Returns:
///     Forecasted per-period volatility (float >= 0).
#[pyfunction]
#[pyo3(signature = (returns, p=1, q=1, mean="zero".to_string()))]
pub fn py_garch_forecast(returns: Vec<f64>, p: usize, q: usize, mean: String) -> f64 {
    garch::garch_forecast(&returns, p, q, &mean)
}
