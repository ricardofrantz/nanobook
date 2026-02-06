use nanobook::portfolio::{CostModel, Portfolio};
use pyo3::prelude::*;
use pyo3::types::PyDict;

use crate::metrics::PyMetrics;
use crate::multi::PyMultiExchange;
use crate::position::PyPosition;
use crate::types::parse_symbol;

/// Transaction cost model.
///
/// Args:
///     commission_bps: Commission in basis points (1 bps = 0.01%)
///     slippage_bps: Slippage estimate in basis points
///     min_trade_fee: Minimum fee per trade in cents
///
/// Example::
///
///     model = CostModel(commission_bps=10, slippage_bps=5, min_trade_fee=100)
///     zero = CostModel.zero()
///
#[pyclass(name = "CostModel")]
#[derive(Clone)]
pub struct PyCostModel {
    pub inner: CostModel,
}

#[pymethods]
impl PyCostModel {
    #[new]
    #[pyo3(signature = (commission_bps=0, slippage_bps=0, min_trade_fee=0))]
    fn new(commission_bps: u32, slippage_bps: u32, min_trade_fee: i64) -> Self {
        Self {
            inner: CostModel {
                commission_bps,
                slippage_bps,
                min_trade_fee,
            },
        }
    }

    /// Create a zero-cost model.
    #[staticmethod]
    fn zero() -> Self {
        Self {
            inner: CostModel::zero(),
        }
    }

    /// Compute cost for a trade with the given notional value (cents).
    fn compute_cost(&self, notional: i64) -> i64 {
        self.inner.compute_cost(notional)
    }

    fn __repr__(&self) -> String {
        format!(
            "CostModel(commission_bps={}, slippage_bps={}, min_trade_fee={})",
            self.inner.commission_bps, self.inner.slippage_bps, self.inner.min_trade_fee
        )
    }
}

/// Portfolio: tracks cash, positions, and returns.
///
/// Args:
///     initial_cash: Starting cash in cents (e.g., 1_000_000_00 = $1M)
///     cost_model: A CostModel instance
///
/// Example::
///
///     portfolio = Portfolio(1_000_000_00, CostModel.zero())
///     portfolio.rebalance_simple([("AAPL", 0.6)], [("AAPL", 15000)])
///
#[pyclass(name = "Portfolio")]
#[derive(Clone)]
pub struct PyPortfolio {
    pub inner: Portfolio,
}

impl PyPortfolio {
    pub fn from_portfolio(inner: Portfolio) -> Self {
        Self { inner }
    }
}

#[pymethods]
impl PyPortfolio {
    #[new]
    fn new(initial_cash: i64, cost_model: &PyCostModel) -> Self {
        Self {
            inner: Portfolio::new(initial_cash, cost_model.inner),
        }
    }

    /// Current cash balance in cents.
    #[getter]
    fn cash(&self) -> i64 {
        self.inner.cash()
    }

    /// Get a position by symbol.
    fn position(&self, symbol: &str) -> PyResult<Option<PyPosition>> {
        let sym = parse_symbol(symbol)?;
        Ok(self
            .inner
            .position(&sym)
            .map(|p| PyPosition { inner: p.clone() }))
    }

    /// Get all positions as a dict {symbol: Position}.
    fn positions(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        for (sym, pos) in self.inner.positions() {
            dict.set_item(sym.to_string(), PyPosition { inner: pos.clone() })?;
        }
        Ok(dict.into())
    }

    /// Total equity (cash + position values) given current prices.
    ///
    /// Args:
    ///     prices: List of (symbol, price_in_cents) tuples
    fn total_equity(&self, prices: Vec<(String, i64)>) -> PyResult<i64> {
        let prices = parse_price_list(&prices)?;
        Ok(self.inner.total_equity(&prices))
    }

    /// Current portfolio weights.
    ///
    /// Returns list of (symbol, weight) tuples.
    fn current_weights(&self, prices: Vec<(String, i64)>) -> PyResult<Vec<(String, f64)>> {
        let prices = parse_price_list(&prices)?;
        Ok(self
            .inner
            .current_weights(&prices)
            .into_iter()
            .map(|(sym, w)| (sym.as_str().to_string(), w))
            .collect())
    }

    /// Get the return series.
    fn returns(&self) -> Vec<f64> {
        self.inner.returns().to_vec()
    }

    /// Get the equity curve.
    fn equity_curve(&self) -> Vec<i64> {
        self.inner.equity_curve().to_vec()
    }

    /// Rebalance to target weights using simple fill (instant execution).
    ///
    /// Args:
    ///     targets: List of (symbol, weight) tuples. Weights should sum to <= 1.0.
    ///     prices: List of (symbol, price_in_cents) tuples.
    fn rebalance_simple(
        &mut self,
        targets: Vec<(String, f64)>,
        prices: Vec<(String, i64)>,
    ) -> PyResult<()> {
        let targets = parse_target_list(&targets)?;
        let prices = parse_price_list(&prices)?;
        self.inner.rebalance_simple(&targets, &prices);
        Ok(())
    }

    /// Rebalance through LOB matching engines.
    fn rebalance_lob(
        &mut self,
        targets: Vec<(String, f64)>,
        exchanges: &mut PyMultiExchange,
    ) -> PyResult<()> {
        let targets = parse_target_list(&targets)?;
        self.inner.rebalance_lob(&targets, &mut exchanges.inner);
        Ok(())
    }

    /// Record a return for the current period.
    fn record_return(&mut self, prices: Vec<(String, i64)>) -> PyResult<()> {
        let prices = parse_price_list(&prices)?;
        self.inner.record_return(&prices);
        Ok(())
    }

    /// Take a portfolio snapshot.
    fn snapshot(&self, py: Python<'_>, prices: Vec<(String, i64)>) -> PyResult<PyObject> {
        let prices = parse_price_list(&prices)?;
        let snap = self.inner.snapshot(&prices);

        let dict = PyDict::new(py);
        dict.set_item("cash", snap.cash)?;
        dict.set_item("equity", snap.equity)?;
        dict.set_item("num_positions", snap.num_positions)?;
        dict.set_item("total_realized_pnl", snap.total_realized_pnl)?;

        let weights = PyDict::new(py);
        for (sym, w) in snap.weights {
            weights.set_item(sym.to_string(), w)?;
        }
        dict.set_item("weights", weights)?;

        Ok(dict.into())
    }

    /// Compute metrics from the recorded return series.
    ///
    /// Args:
    ///     periods_per_year: Annualization factor (252 for daily, 12 for monthly)
    ///     risk_free: Risk-free rate per period
    fn compute_metrics(&self, periods_per_year: f64, risk_free: f64) -> Option<PyMetrics> {
        nanobook::portfolio::compute_metrics(self.inner.returns(), periods_per_year, risk_free)
            .map(PyMetrics::from)
    }

    /// Save portfolio state to a JSON file.
    fn save_json(&self, path: &str) -> PyResult<()> {
        self.inner
            .save_json(std::path::Path::new(path))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))
    }

    /// Load portfolio state from a JSON file.
    #[staticmethod]
    fn load_json(path: &str) -> PyResult<Self> {
        let inner = Portfolio::load_json(std::path::Path::new(path))
            .map_err(|e| pyo3::exceptions::PyIOError::new_err(e.to_string()))?;
        Ok(Self { inner })
    }

    fn __repr__(&self) -> String {
        format!(
            "Portfolio(cash=${:.2}, returns={})",
            self.inner.cash() as f64 / 100.0,
            self.inner.returns().len()
        )
    }
}

/// Parse Python list of (str, i64) into Vec<(Symbol, i64)>.
fn parse_price_list(prices: &[(String, i64)]) -> PyResult<Vec<(nanobook::Symbol, i64)>> {
    prices
        .iter()
        .map(|(s, p)| Ok((parse_symbol(s)?, *p)))
        .collect()
}

/// Parse Python list of (str, f64) into Vec<(Symbol, f64)>.
fn parse_target_list(targets: &[(String, f64)]) -> PyResult<Vec<(nanobook::Symbol, f64)>> {
    targets
        .iter()
        .map(|(s, w)| Ok((parse_symbol(s)?, *w)))
        .collect()
}
