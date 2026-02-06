use nanobook::portfolio::sweep::sweep_strategy;
use nanobook::portfolio::{CostModel, EqualWeight};
use pyo3::prelude::*;

use crate::metrics::PyMetrics;
use crate::types::parse_symbol;

/// Run a parallel parameter sweep using the EqualWeight strategy.
///
/// This releases the GIL during computation, so Python threads
/// can run while Rayon does parallel work.
///
/// Args:
///     n_params: Number of parameter configurations to sweep
///     price_series: List of bars, each bar is [(symbol, price_cents), ...]
///     initial_cash: Starting cash in cents
///     periods_per_year: Annualization factor
///     risk_free: Risk-free rate per period
///
/// Returns:
///     List of Metrics (one per parameter)
///
/// Example::
///
///     results = sweep_equal_weight(
///         100,
///         prices,
///         1_000_000_00,
///         12.0,
///         0.0,
///     )
///
#[pyfunction]
#[pyo3(signature = (n_params, price_series, initial_cash, periods_per_year=12.0, risk_free=0.0))]
pub fn py_sweep_equal_weight(
    py: Python<'_>,
    n_params: usize,
    price_series: Vec<Vec<(String, i64)>>,
    initial_cash: i64,
    periods_per_year: f64,
    risk_free: f64,
) -> PyResult<Vec<Option<PyMetrics>>> {
    // Convert price series upfront (before releasing GIL)
    let price_series: Vec<Vec<(nanobook::Symbol, i64)>> = price_series
        .into_iter()
        .map(|bar| {
            bar.into_iter()
                .map(|(s, p)| Ok((parse_symbol(&s)?, p)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;

    let params: Vec<usize> = (0..n_params).collect();

    // Release the GIL for Rayon parallel execution
    let results = py.allow_threads(|| {
        sweep_strategy(
            &params,
            &price_series,
            initial_cash,
            CostModel::zero(),
            periods_per_year,
            risk_free,
            |_| EqualWeight,
        )
    });

    Ok(results
        .into_iter()
        .map(|r| r.metrics.map(PyMetrics::from))
        .collect())
}
