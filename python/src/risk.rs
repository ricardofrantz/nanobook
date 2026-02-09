//! PyO3 bindings for the risk engine.

use nanobook_broker::{Account, BrokerSide};
use nanobook_risk::{RiskConfig, RiskEngine as RustRiskEngine};
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::types::parse_symbol;

/// Pre-trade risk engine.
///
/// Validates orders against configurable risk limits before execution.
///
/// Args:
///     max_position_pct: Max single-position weight (e.g., 0.20 = 20%)
///     max_leverage: Max gross leverage (1.0 for long-only)
///     max_drawdown_pct: Max drawdown circuit breaker
///     allow_short: Whether short selling is allowed
///     max_short_pct: Max short exposure as fraction of equity
///     max_trade_usd: Max single trade size in USD
///
/// Example::
///
///     risk = RiskEngine(max_position_pct=0.20, max_leverage=1.0, max_drawdown_pct=0.15)
///
#[pyclass(name = "RiskEngine")]
pub struct PyRiskEngine {
    inner: RustRiskEngine,
}

#[pymethods]
impl PyRiskEngine {
    #[new]
    #[pyo3(signature = (
        max_position_pct=0.25,
        max_leverage=1.5,
        max_drawdown_pct=0.20,
        allow_short=true,
        max_short_pct=0.30,
        max_trade_usd=100_000.0,
    ))]
    fn new(
        max_position_pct: f64,
        max_leverage: f64,
        max_drawdown_pct: f64,
        allow_short: bool,
        max_short_pct: f64,
        max_trade_usd: f64,
    ) -> Self {
        let config = RiskConfig {
            max_position_pct,
            max_leverage,
            max_drawdown_pct,
            allow_short,
            max_short_pct,
            max_trade_usd,
            ..RiskConfig::default()
        };
        Self {
            inner: RustRiskEngine::new(config),
        }
    }

    /// Check a single order against risk limits.
    ///
    /// Args:
    ///     symbol: Ticker symbol
    ///     side: "buy" or "sell"
    ///     quantity: Number of shares
    ///     price_cents: Price in cents
    ///     equity_cents: Account equity in cents
    ///     positions: List of (symbol, quantity) tuples for current positions
    ///
    /// Returns a list of dicts with keys: name, status, detail.
    #[allow(clippy::too_many_arguments)]
    fn check_order(
        &self,
        py: Python<'_>,
        symbol: &str,
        side: &str,
        quantity: u64,
        price_cents: i64,
        equity_cents: i64,
        positions: Vec<(String, i64)>,
    ) -> PyResult<PyObject> {
        let sym = parse_symbol(symbol)?;
        let broker_side = parse_side(side)?;

        let account = Account {
            equity_cents,
            buying_power_cents: equity_cents,
            cash_cents: equity_cents,
            gross_position_value_cents: 0,
        };

        let pos: Vec<_> = positions
            .iter()
            .map(|(s, q)| Ok((parse_symbol(s)?, *q)))
            .collect::<PyResult<Vec<_>>>()?;

        let report =
            self.inner
                .check_order(&sym, broker_side, quantity, price_cents, &account, &pos);

        report_to_py(py, &report)
    }

    /// Check a batch of orders against risk limits.
    ///
    /// Args:
    ///     orders: List of (symbol, side, quantity, price_cents) tuples
    ///     equity_cents: Account equity in cents
    ///     positions: List of (symbol, quantity) tuples for current positions
    ///     target_weights: List of (symbol, weight) tuples for targets
    ///
    /// Returns a list of dicts with keys: name, status, detail.
    fn check_batch(
        &self,
        py: Python<'_>,
        orders: Vec<(String, String, u64, i64)>,
        equity_cents: i64,
        positions: Vec<(String, i64)>,
        target_weights: Vec<(String, f64)>,
    ) -> PyResult<PyObject> {
        let account = Account {
            equity_cents,
            buying_power_cents: equity_cents,
            cash_cents: equity_cents,
            gross_position_value_cents: 0,
        };

        let broker_orders: Vec<_> = orders
            .iter()
            .map(|(s, side, qty, price)| Ok((parse_symbol(s)?, parse_side(side)?, *qty, *price)))
            .collect::<PyResult<Vec<_>>>()?;

        let pos: Vec<_> = positions
            .iter()
            .map(|(s, q)| Ok((parse_symbol(s)?, *q)))
            .collect::<PyResult<Vec<_>>>()?;

        let targets: Vec<_> = target_weights
            .iter()
            .map(|(s, w)| Ok((parse_symbol(s)?, *w)))
            .collect::<PyResult<Vec<_>>>()?;

        let report = self
            .inner
            .check_batch(&broker_orders, &account, &pos, &targets);

        report_to_py(py, &report)
    }

    fn __repr__(&self) -> String {
        let config = self.inner.config();
        format!(
            "RiskEngine(max_position_pct={}, max_leverage={}, allow_short={})",
            config.max_position_pct, config.max_leverage, config.allow_short,
        )
    }
}

fn parse_side(s: &str) -> PyResult<BrokerSide> {
    match s.to_ascii_lowercase().as_str() {
        "buy" | "b" => Ok(BrokerSide::Buy),
        "sell" | "s" => Ok(BrokerSide::Sell),
        _ => Err(pyo3::exceptions::PyValueError::new_err(format!(
            "Invalid side '{s}'. Use 'buy' or 'sell'."
        ))),
    }
}

fn report_to_py(py: Python<'_>, report: &nanobook_risk::RiskReport) -> PyResult<PyObject> {
    let list = PyList::empty(py);
    for check in &report.checks {
        let dict = PyDict::new(py);
        dict.set_item("name", check.name)?;
        dict.set_item("status", format!("{}", check.status))?;
        dict.set_item("detail", &check.detail)?;
        list.append(dict)?;
    }
    Ok(list.into())
}
