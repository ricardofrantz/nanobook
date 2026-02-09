//! PyO3 binding for the fast backtest bridge.

use nanobook::backtest_bridge;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::metrics::PyMetrics;
use crate::types::parse_symbol;

/// Simulate portfolio returns from a pre-computed weight schedule.
///
/// Python computes weights (factors, signals), Rust simulates the execution loop.
///
/// Args:
///     weight_schedule: List of weight dicts, one per period.
///         Each dict maps symbol (str) to weight (float).
///     price_schedule: List of price dicts, one per period (parallel with weight_schedule).
///         Each dict maps symbol (str) to price_cents (int).
///     initial_cash: Starting cash in cents (e.g., 1_000_000_00 = $1M).
///     cost_bps: Transaction cost in basis points (e.g., 15).
///     periods_per_year: Annualization factor (252 for daily, 12 for monthly).
///     risk_free: Risk-free rate per period.
///
/// Returns a dict with keys: returns, equity_curve, final_cash, metrics.
///
/// Example::
///
///     result = nanobook.backtest_weights(
///         weight_schedule=[
///             {"AAPL": 0.5, "MSFT": 0.5},
///             {"AAPL": 0.3, "NVDA": 0.7},
///         ],
///         price_schedule=[
///             {"AAPL": 185_00, "MSFT": 370_00},
///             {"AAPL": 190_00, "MSFT": 380_00, "NVDA": 600_00},
///         ],
///         initial_cash=1_000_000_00,
///         cost_bps=15,
///     )
///     print(f"Sharpe: {result['metrics'].sharpe:.2f}")
///
#[pyfunction]
#[pyo3(signature = (weight_schedule, price_schedule, initial_cash, cost_bps, periods_per_year=252.0, risk_free=0.0))]
pub fn py_backtest_weights(
    py: Python<'_>,
    weight_schedule: Vec<Vec<(String, f64)>>,
    price_schedule: Vec<Vec<(String, i64)>>,
    initial_cash: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
) -> PyResult<PyObject> {
    // Convert Python types to Rust types
    let rust_weights: Vec<Vec<(nanobook::Symbol, f64)>> = weight_schedule
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, w)| Ok((parse_symbol(s)?, *w)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;

    let rust_prices: Vec<Vec<(nanobook::Symbol, i64)>> = price_schedule
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, p)| Ok((parse_symbol(s)?, *p)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;

    // Release GIL during computation
    let result = py.allow_threads(|| {
        backtest_bridge::backtest_weights(
            &rust_weights,
            &rust_prices,
            initial_cash,
            cost_bps,
            periods_per_year,
            risk_free,
        )
    });

    // Convert result to Python dict
    let dict = PyDict::new(py);
    dict.set_item("returns", result.returns)?;
    dict.set_item("equity_curve", result.equity_curve)?;
    dict.set_item("final_cash", result.final_cash)?;
    dict.set_item("metrics", result.metrics.map(PyMetrics::from))?;

    Ok(dict.into())
}
