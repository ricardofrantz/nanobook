mod backtest_bridge;
mod broker;
mod cv;
mod event;
mod exchange;
mod indicators;
#[cfg(feature = "itch")]
mod itch;
mod metrics;
mod multi;
mod order;
mod portfolio;
mod position;
mod results;
mod risk;
mod stats;
mod strategy;
mod sweep;
mod types;

use pyo3::prelude::*;

/// nanobook: Python bindings for a deterministic limit order book
/// and matching engine for testing trading algorithms.
#[pymodule]
fn nanobook(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", "0.8.0")?;

    // Broker types
    m.add_class::<broker::PyIbkrBroker>()?;
    m.add_class::<broker::PyBinanceBroker>()?;

    // Risk engine
    m.add_class::<risk::PyRiskEngine>()?;

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

    // v0.7 functions
    m.add_function(wrap_pyfunction!(metrics::py_compute_metrics, m)?)?;
    m.add_function(wrap_pyfunction!(sweep::py_sweep_equal_weight, m)?)?;
    m.add_function(wrap_pyfunction!(strategy::py_run_backtest, m)?)?;
    m.add_function(wrap_pyfunction!(backtest_bridge::py_backtest_weights, m)?)?;
    #[cfg(feature = "itch")]
    m.add_function(wrap_pyfunction!(itch::parse_itch, m)?)?;

    // v0.8 — Technical indicators (ta-lib replacements)
    m.add_function(wrap_pyfunction!(indicators::py_rsi, m)?)?;
    m.add_function(wrap_pyfunction!(indicators::py_macd, m)?)?;
    m.add_function(wrap_pyfunction!(indicators::py_bbands, m)?)?;
    m.add_function(wrap_pyfunction!(indicators::py_atr, m)?)?;

    // v0.8 — Statistics (scipy replacements)
    m.add_function(wrap_pyfunction!(stats::py_spearman, m)?)?;
    m.add_function(wrap_pyfunction!(stats::py_quintile_spread, m)?)?;

    // v0.8 — Cross-validation (sklearn replacement)
    m.add_function(wrap_pyfunction!(cv::py_time_series_split, m)?)?;

    // v0.8 — Rolling metrics (quantstats replacements)
    m.add_function(wrap_pyfunction!(metrics::py_rolling_sharpe, m)?)?;
    m.add_function(wrap_pyfunction!(metrics::py_rolling_volatility, m)?)?;

    Ok(())
}
