use nanobook::garch;
use pyo3::prelude::*;

fn warn_deprecated(py: Python<'_>, message: &str) -> PyResult<()> {
    let warnings = py.import("warnings")?;
    let builtins = py.import("builtins")?;
    let category = builtins.getattr("DeprecationWarning")?;
    warnings.call_method1("warn", (message, category, 2))?;
    Ok(())
}

/// One-step-ahead EWMA-style volatility forecast with fixed parameters.
///
/// Args:
///     returns: Return series as decimal fractions.
///     p: ARCH lag count (default 1).
///     q: variance-recursion lag count (default 1).
///     mean: Mean model, ``"zero"`` or ``"constant"`` (default ``"zero"``).
///
/// Returns:
///     Forecasted per-period volatility (float >= 0).
#[pyfunction]
#[pyo3(signature = (returns, p=1, q=1, mean="zero".to_string()))]
pub fn garch_ewma_forecast(returns: Vec<f64>, p: usize, q: usize, mean: String) -> f64 {
    garch::garch_ewma_forecast(&returns, p, q, &mean)
}

#[pyfunction]
#[pyo3(signature = (returns, p=1, q=1, mean="zero".to_string()))]
pub fn py_garch_ewma_forecast(returns: Vec<f64>, p: usize, q: usize, mean: String) -> f64 {
    garch_ewma_forecast(returns, p, q, mean)
}

#[pyfunction]
#[pyo3(signature = (returns, p=1, q=1, mean="zero".to_string()))]
pub fn garch_forecast(
    py: Python<'_>,
    returns: Vec<f64>,
    p: usize,
    q: usize,
    mean: String,
) -> PyResult<f64> {
    warn_deprecated(
        py,
        "nanobook.garch_forecast is deprecated; use garch_ewma_forecast",
    )?;
    Ok(garch::garch_ewma_forecast(&returns, p, q, &mean))
}

#[pyfunction]
#[pyo3(signature = (returns, p=1, q=1, mean="zero".to_string()))]
pub fn py_garch_forecast(
    py: Python<'_>,
    returns: Vec<f64>,
    p: usize,
    q: usize,
    mean: String,
) -> PyResult<f64> {
    warn_deprecated(
        py,
        "nanobook.py_garch_forecast is deprecated; use py_garch_ewma_forecast",
    )?;
    Ok(garch::garch_ewma_forecast(&returns, p, q, &mean))
}
