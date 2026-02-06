use nanobook::portfolio::{compute_metrics, Metrics};
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
}

#[pymethods]
impl PyMetrics {
    fn __repr__(&self) -> String {
        format!(
            "Metrics(total_return={:.2}%, sharpe={:.2}, max_drawdown={:.2}%)",
            self.total_return * 100.0,
            self.sharpe,
            self.max_drawdown * 100.0,
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
///     m = compute_metrics([0.01, -0.005, 0.02], 252.0, 0.0)
///     print(m.sharpe)
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
