use nanobook::Symbol;
use nanobook::portfolio::{Portfolio, Strategy, run_backtest};
use pyo3::prelude::*;
use std::collections::HashMap;

use crate::portfolio::{PyCostModel, PyPortfolio};
use crate::results::PyBacktestResult;
use crate::types::parse_symbol;

pub struct PyStrategy {
    pub callback: Py<PyAny>,
}

impl Strategy for PyStrategy {
    fn compute_weights(
        &self,
        bar_index: usize,
        prices: &[(Symbol, i64)],
        portfolio: &Portfolio,
    ) -> Vec<(Symbol, f64)> {
        Python::with_gil(|py| {
            let py_prices: HashMap<String, i64> = prices
                .iter()
                .map(|(sym, p)| (sym.to_string(), *p))
                .collect();

            let py_portfolio = PyPortfolio::from_portfolio(portfolio.clone());

            let args = (bar_index, py_prices, py_portfolio);
            let result = self.callback.call1(py, args);

            match result {
                Ok(obj) => {
                    let weights: Vec<(String, f64)> = match obj.extract(py) {
                        Ok(w) => w,
                        Err(_) => Vec::new(),
                    };

                    weights
                        .into_iter()
                        .filter_map(|(s, w)| Symbol::try_new(&s).map(|sym| (sym, w)))
                        .collect()
                }
                Err(e) => {
                    e.print(py);
                    Vec::new()
                }
            }
        })
    }
}

#[pyfunction]
#[pyo3(name = "run_backtest")]
#[pyo3(signature = (strategy, price_series, initial_cash, cost_model, periods_per_year=252.0, risk_free=0.0))]
pub fn py_run_backtest(
    strategy: Py<PyAny>,
    price_series: Vec<HashMap<String, i64>>,
    initial_cash: i64,
    cost_model: PyCostModel,
    periods_per_year: f64,
    risk_free: f64,
) -> PyResult<PyBacktestResult> {
    let strat = PyStrategy { callback: strategy };

    let mut rust_series = Vec::with_capacity(price_series.len());
    for bar in price_series {
        let mut rust_bar = Vec::with_capacity(bar.len());
        for (s, p) in bar {
            rust_bar.push((parse_symbol(&s)?, p));
        }
        rust_series.push(rust_bar);
    }

    let result = run_backtest(
        &strat,
        &rust_series,
        initial_cash,
        cost_model.inner,
        periods_per_year,
        risk_free,
    );

    Ok(result.into())
}
