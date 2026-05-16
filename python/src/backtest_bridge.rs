//! PyO3 binding for the fast backtest bridge.

use nanobook::backtest_bridge::{self, BacktestBridgeOptions, BacktestStopConfig};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

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
///     stop_cfg: Optional stop simulation config dictionary with supported keys:
///         ``fixed_stop_pct``, ``trailing_stop_pct``, ``atr_multiple``, ``atr_period``.
///
/// Returns a dict with keys:
///     ``returns``, ``equity_curve``, ``final_cash``, ``metrics``, ``holdings``,
///     ``symbol_returns``, ``stop_events``.
#[pyfunction]
#[pyo3(signature = (weight_schedule, price_schedule, initial_cash, cost_bps, periods_per_year=252.0, risk_free=0.0, stop_cfg=None))]
#[allow(clippy::too_many_arguments)]
pub fn backtest_weights(
    py: Python<'_>,
    weight_schedule: Vec<Vec<(String, f64)>>,
    price_schedule: Vec<Vec<(String, i64)>>,
    initial_cash: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
    stop_cfg: Option<Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    // Convert Python types to Rust types.
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

    let options = BacktestBridgeOptions {
        stop_cfg: parse_stop_cfg(stop_cfg)?,
    };

    // Release GIL during computation.
    let result = py.detach(|| {
        backtest_bridge::backtest_weights_with_options(
            &rust_weights,
            &rust_prices,
            initial_cash,
            cost_bps,
            periods_per_year,
            risk_free,
            options,
        )
    });

    // Convert result to Python dict.
    let dict = PyDict::new(py);
    dict.set_item("returns", result.returns)?;
    dict.set_item("equity_curve", result.equity_curve)?;
    dict.set_item("final_cash", result.final_cash)?;
    dict.set_item("metrics", result.metrics.map(PyMetrics::from))?;

    let holdings: Vec<Vec<(String, f64)>> = result
        .holdings
        .into_iter()
        .map(|period| {
            period
                .into_iter()
                .map(|(s, w)| (s.to_string(), w))
                .collect()
        })
        .collect();
    dict.set_item("holdings", holdings)?;

    let symbol_returns: Vec<Vec<(String, f64)>> = result
        .symbol_returns
        .into_iter()
        .map(|period| {
            period
                .into_iter()
                .map(|(s, r)| (s.to_string(), r))
                .collect()
        })
        .collect();
    dict.set_item("symbol_returns", symbol_returns)?;

    let stop_events = PyList::empty(py);
    for ev in result.stop_events {
        let item = PyDict::new(py);
        item.set_item("period_index", ev.period_index)?;
        item.set_item("symbol", ev.symbol.to_string())?;
        item.set_item("trigger_price", ev.trigger_price)?;
        item.set_item("exit_price", ev.exit_price)?;
        item.set_item("reason", ev.reason)?;
        stop_events.append(item)?;
    }
    dict.set_item("stop_events", stop_events)?;

    Ok(dict.into_any().unbind())
}

/// Decompose a weight/return schedule into per-symbol contribution curves and trades.
#[pyfunction]
pub fn py_decompose_backtest(
    py: Python<'_>,
    weight_schedule: Vec<Vec<(String, f64)>>,
    return_schedule: Vec<Vec<(String, f64)>>,
) -> PyResult<Py<PyAny>> {
    let rust_weights: Vec<Vec<(nanobook::Symbol, f64)>> = weight_schedule
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, w)| Ok((parse_symbol(s)?, *w)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;
    let rust_returns: Vec<Vec<(nanobook::Symbol, f64)>> = return_schedule
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, r)| Ok((parse_symbol(s)?, *r)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;

    let result = py.detach(|| backtest_bridge::decompose_backtest(&rust_weights, &rust_returns));
    let dict = PyDict::new(py);

    let contributions: Vec<Vec<(String, f64)>> = result
        .contributions
        .into_iter()
        .map(|period| {
            period
                .into_iter()
                .map(|(s, v)| (s.to_string(), v))
                .collect()
        })
        .collect();
    dict.set_item("contributions", contributions)?;

    let cumulative_contributions: Vec<Vec<(String, f64)>> = result
        .cumulative_contributions
        .into_iter()
        .map(|period| {
            period
                .into_iter()
                .map(|(s, v)| (s.to_string(), v))
                .collect()
        })
        .collect();
    dict.set_item("cumulative_contributions", cumulative_contributions)?;

    let trades = PyList::empty(py);
    for trade in result.trades {
        let item = PyDict::new(py);
        item.set_item("symbol", trade.symbol.to_string())?;
        item.set_item("entry_index", trade.entry_index)?;
        item.set_item("exit_index", trade.exit_index)?;
        item.set_item("entry_weight", trade.entry_weight)?;
        item.set_item("exit_weight", trade.exit_weight)?;
        trades.append(item)?;
    }
    dict.set_item("trades", trades)?;

    Ok(dict.into_any().unbind())
}

/// Build a reporting payload from ``backtest_weights`` output.
#[pyfunction]
#[pyo3(signature = (backtest_result, rolling_window=63, periods_per_year=252))]
pub fn py_tear_sheet(
    py: Python<'_>,
    backtest_result: Bound<'_, PyDict>,
    rolling_window: usize,
    periods_per_year: usize,
) -> PyResult<Py<PyAny>> {
    let returns: Vec<f64> = backtest_result
        .get_item("returns")?
        .ok_or_else(|| PyValueError::new_err("backtest_result missing returns"))?
        .extract()?;
    let equity_curve: Vec<i64> = backtest_result
        .get_item("equity_curve")?
        .ok_or_else(|| PyValueError::new_err("backtest_result missing equity_curve"))?
        .extract()?;
    let holdings_raw: Vec<Vec<(String, f64)>> = backtest_result
        .get_item("holdings")?
        .ok_or_else(|| PyValueError::new_err("backtest_result missing holdings"))?
        .extract()?;
    let symbol_returns_raw: Vec<Vec<(String, f64)>> = backtest_result
        .get_item("symbol_returns")?
        .ok_or_else(|| PyValueError::new_err("backtest_result missing symbol_returns"))?
        .extract()?;

    let holdings = holdings_raw
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, w)| Ok((parse_symbol(s)?, *w)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;
    let symbol_returns = symbol_returns_raw
        .iter()
        .map(|period| {
            period
                .iter()
                .map(|(s, r)| Ok((parse_symbol(s)?, *r)))
                .collect::<PyResult<Vec<_>>>()
        })
        .collect::<PyResult<Vec<_>>>()?;

    let result = backtest_bridge::BacktestBridgeResult {
        returns,
        equity_curve,
        final_cash: backtest_result
            .get_item("final_cash")?
            .and_then(|v| v.extract().ok())
            .unwrap_or(0),
        metrics: None,
        holdings,
        symbol_returns,
        stop_events: Vec::new(),
    };
    let sheet =
        py.detach(|| backtest_bridge::tear_sheet(&result, rolling_window, periods_per_year));

    let dict = PyDict::new(py);
    dict.set_item("monthly_returns", sheet.monthly_returns)?;
    dict.set_item("rolling_sharpe", sheet.rolling_sharpe)?;
    let trade_analytics = PyDict::new(py);
    trade_analytics.set_item("trade_count", sheet.trade_analytics.trade_count)?;
    trade_analytics.set_item("open_trade_count", sheet.trade_analytics.open_trade_count)?;
    trade_analytics.set_item(
        "closed_trade_count",
        sheet.trade_analytics.closed_trade_count,
    )?;
    dict.set_item("trade_analytics", trade_analytics)?;
    let drawdowns = PyList::empty(py);
    for event in sheet.drawdown_events {
        let item = PyDict::new(py);
        item.set_item("drawdown_pct", event.drawdown_pct)?;
        item.set_item("underwater_periods", event.underwater_periods)?;
        item.set_item("peak_index", event.peak_index)?;
        item.set_item("trough_index", event.trough_index)?;
        item.set_item("recovery_index", event.recovery_index)?;
        drawdowns.append(item)?;
    }
    dict.set_item("drawdown_events", drawdowns)?;
    Ok(dict.into_any().unbind())
}

/// Backward-compatible alias for older callers using ``py_backtest_weights``.
#[pyfunction]
#[pyo3(signature = (weight_schedule, price_schedule, initial_cash, cost_bps, periods_per_year=252.0, risk_free=0.0, stop_cfg=None))]
#[allow(clippy::too_many_arguments)]
pub fn py_backtest_weights(
    py: Python<'_>,
    weight_schedule: Vec<Vec<(String, f64)>>,
    price_schedule: Vec<Vec<(String, i64)>>,
    initial_cash: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
    stop_cfg: Option<Bound<'_, PyDict>>,
) -> PyResult<Py<PyAny>> {
    backtest_weights(
        py,
        weight_schedule,
        price_schedule,
        initial_cash,
        cost_bps,
        periods_per_year,
        risk_free,
        stop_cfg,
    )
}

fn parse_stop_cfg(stop_cfg: Option<Bound<'_, PyDict>>) -> PyResult<Option<BacktestStopConfig>> {
    let Some(cfg) = stop_cfg else {
        return Ok(None);
    };

    let fixed_stop_pct = extract_opt_f64(&cfg, "fixed_stop_pct")?;
    let trailing_stop_pct = extract_opt_f64(&cfg, "trailing_stop_pct")?;
    let atr_multiple = extract_opt_f64(&cfg, "atr_multiple")?;

    let atr_period: usize = match cfg.get_item("atr_period")? {
        Some(v) => v.extract()?,
        None => 14,
    };

    if let Some(v) = fixed_stop_pct
        && (!(0.0..1.0).contains(&v) || !v.is_finite())
    {
        return Err(PyValueError::new_err(
            "fixed_stop_pct must be finite and in (0, 1)",
        ));
    }

    if let Some(v) = trailing_stop_pct
        && (!(0.0..1.0).contains(&v) || !v.is_finite())
    {
        return Err(PyValueError::new_err(
            "trailing_stop_pct must be finite and in (0, 1)",
        ));
    }

    if let Some(v) = atr_multiple
        && (v <= 0.0 || !v.is_finite())
    {
        return Err(PyValueError::new_err("atr_multiple must be finite and > 0"));
    }

    if atr_period == 0 {
        return Err(PyValueError::new_err("atr_period must be >= 1"));
    }

    Ok(Some(BacktestStopConfig {
        fixed_stop_pct,
        trailing_stop_pct,
        atr_multiple,
        atr_period,
    }))
}

fn extract_opt_f64(cfg: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<f64>> {
    match cfg.get_item(key)? {
        Some(v) => Ok(Some(v.extract()?)),
        None => Ok(None),
    }
}
