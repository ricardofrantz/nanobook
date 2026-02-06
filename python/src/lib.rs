mod exchange;
mod event;
mod metrics;
mod multi;
mod order;
mod portfolio;
mod position;
mod results;
mod strategy;
mod sweep;
mod types;

use pyo3::prelude::*;

/// nanobook: Python bindings for a deterministic limit order book
/// and matching engine for testing trading algorithms.
#[pymodule]
fn nanobook(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "0.5.0")?;

    // Core exchange types
    m.add_class::<exchange::PyExchange>()?;
    m.add_class::<multi::PyMultiExchange>()?;
    m.add_class::<order::PyOrder>()?;
    m.add_class::<event::PyEvent>()?;

    // Result types
    m.add_class::<results::PySubmitResult>()?;
    m.add_class::<results::PyCancelResult>()?;
    m.add_class::<results::PyModifyResult>()?;
    m.add_class::<results::PyStopSubmitResult>()?;
    m.add_class::<results::PyTrade>()?;
    m.add_class::<results::PyLevelSnapshot>()?;
    m.add_class::<exchange::PyBookSnapshot>()?;
    m.add_class::<results::PyBacktestResult>()?;

    // Portfolio types
    m.add_class::<portfolio::PyCostModel>()?;
    m.add_class::<portfolio::PyPortfolio>()?;
    m.add_class::<position::PyPosition>()?;
    m.add_class::<metrics::PyMetrics>()?;

    // Functions
    m.add_function(wrap_pyfunction!(metrics::py_compute_metrics, m)?)?;
    m.add_function(wrap_pyfunction!(sweep::py_sweep_equal_weight, m)?)?;
    m.add_function(wrap_pyfunction!(strategy::py_run_backtest, m)?)?;

    Ok(())
}
