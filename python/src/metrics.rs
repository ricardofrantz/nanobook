use nanobook::portfolio::metrics::{compute_metrics, rolling_sharpe, rolling_volatility, Metrics};
use pyo3::prelude::*;

/// Performance metrics for a return series.
#[pyclass(name = "Metrics")]
#[derive(Clone)]
pub struct PyMetrics {
    #[pyo3(get)]
    pub total_return: f64,
    #[pyo3(get)]
    pub cagr: f64,
    #[pyo3(get)]
    pub volatility: f64,
    #[pyo3(get)]
    pub sharpe: f64,
    #[pyo3(get)]
    pub sortino: f64,
    #[pyo3(get)]
    pub max_drawdown: f64,
    #[pyo3(get)]
    pub calmar: f64,
    #[pyo3(get)]
    pub num_periods: usize,
    #[pyo3(get)]
    pub winning_periods: usize,
    #[pyo3(get)]
    pub losing_periods: usize,

    // v0.8 extended metrics
    #[pyo3(get)]
    pub cvar_95: f64,
    #[pyo3(get)]
    pub win_rate: f64,
    #[pyo3(get)]
    pub profit_factor: f64,
    #[pyo3(get)]
    pub payoff_ratio: f64,
    #[pyo3(get)]
    pub kelly: f64,
}

#[pymethods]
impl PyMetrics {
    fn __repr__(&self) -> String {
        format!(
            "Metrics(total_return={:.2}%, sharpe={:.2}, max_drawdown={:.2}%, win_rate={:.1}%)",
            self.total_return * 100.0,
            self.sharpe,
            self.max_drawdown * 100.0,
            self.win_rate * 100.0,
        )
    }
}

impl From<Metrics> for PyMetrics {
    fn from(m: Metrics) -> Self {
        Self {
            total_return: m.total_return,
            cagr: m.cagr,
            volatility: m.volatility,
            sharpe: m.sharpe,
            sortino: m.sortino,
            max_drawdown: m.max_drawdown,
            calmar: m.calmar,
            num_periods: m.num_periods,
            winning_periods: m.winning_periods,
            losing_periods: m.losing_periods,
            cvar_95: m.cvar_95,
            win_rate: m.win_rate,
            profit_factor: m.profit_factor,
            payoff_ratio: m.payoff_ratio,
            kelly: m.kelly,
        }
    }
}

/// Compute performance metrics from a return series.
///
/// Args:
///     returns: List of periodic returns (e.g., [0.01, -0.005, 0.02])
///     periods_per_year: Annualization factor (252 for daily, 12 for monthly)
///     risk_free: Risk-free rate per period
///
/// Returns:
///     Metrics object, or None if returns is empty
///
/// Example::
///
///     m = nanobook.py_compute_metrics([0.01, -0.005, 0.02], 252.0, 0.0)
///     print(m.sharpe, m.cvar_95, m.kelly)
///
#[pyfunction]
#[pyo3(signature = (returns, periods_per_year=252.0, risk_free=0.0))]
pub fn py_compute_metrics(
    returns: Vec<f64>,
    periods_per_year: f64,
    risk_free: f64,
) -> Option<PyMetrics> {
    compute_metrics(&returns, periods_per_year, risk_free).map(PyMetrics::from)
}

/// Compute rolling Sharpe ratio over a sliding window.
///
/// Args:
///     returns: List of periodic returns.
///     window: Window size (e.g., 63 for quarterly).
///     periods_per_year: Annualization factor (default 252).
///
/// Returns:
///     List of rolling Sharpe values. NaN for incomplete windows.
///
/// Example::
///
///     rolling = nanobook.py_rolling_sharpe(daily_returns, 63, 252)
///
#[pyfunction]
#[pyo3(signature = (returns, window, periods_per_year=252))]
pub fn py_rolling_sharpe(returns: Vec<f64>, window: usize, periods_per_year: usize) -> Vec<f64> {
    rolling_sharpe(&returns, window, periods_per_year)
}

/// Compute rolling annualized volatility over a sliding window.
///
/// Args:
///     returns: List of periodic returns.
///     window: Window size (e.g., 63 for quarterly).
///     periods_per_year: Annualization factor (default 252).
///
/// Returns:
///     List of rolling volatility values. NaN for incomplete windows.
///
/// Example::
///
///     rolling = nanobook.py_rolling_volatility(daily_returns, 63, 252)
///
#[pyfunction]
#[pyo3(signature = (returns, window, periods_per_year=252))]
pub fn py_rolling_volatility(
    returns: Vec<f64>,
    window: usize,
    periods_per_year: usize,
) -> Vec<f64> {
    rolling_volatility(&returns, window, periods_per_year)
}
