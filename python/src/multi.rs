use nanobook::{MultiExchange, OrderId, Price};
use pyo3::prelude::*;

use crate::exchange::PyExchange;
use crate::results::*;
use crate::types::{parse_side, parse_symbol, parse_tif};

/// Multi-symbol exchange wrapping one Exchange per symbol.
///
/// Example::
///
///     multi = MultiExchange()
///     multi.submit_limit("AAPL", "buy", 15000, 100, "gtc")
///
#[pyclass(name = "MultiExchange")]
pub struct PyMultiExchange {
    pub inner: MultiExchange,
}

#[pymethods]
impl PyMultiExchange {
    #[new]
    fn new() -> Self {
        Self {
            inner: MultiExchange::new(),
        }
    }

    /// Get or create an Exchange for the given symbol.
    ///
    /// **Important:** Returns an independent copy of the exchange. Mutations
    /// to the returned ``PyExchange`` do NOT flow back to the ``MultiExchange``.
    /// For mutations, use the ``submit_*`` methods directly on ``MultiExchange``.
    fn get_or_create(&mut self, symbol: &str) -> PyResult<PyExchange> {
        let sym = parse_symbol(symbol)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(PyExchange::from_exchange(ex.clone()))
    }

    /// List all symbols that have exchanges.
    fn symbols(&self) -> Vec<String> {
        self.inner
            .symbols()
            .map(|s| s.as_str().to_string())
            .collect()
    }

    /// Get best bid/ask prices for all symbols.
    /// Returns list of (symbol, bid, ask) tuples.
    fn best_prices(&self) -> Vec<(String, Option<i64>, Option<i64>)> {
        self.inner
            .symbols()
            .map(|sym| {
                let (bid, ask) = self
                    .inner
                    .get(sym)
                    .map(|ex| ex.best_bid_ask())
                    .unwrap_or((None, None));
                (sym.to_string(), bid.map(|p| p.0), ask.map(|p| p.0))
            })
            .collect()
    }

    // === Method Forwarding (Option 3) ===

    #[pyo3(signature = (symbol, side, price, quantity, tif="gtc"))]
    fn submit_limit(
        &mut self,
        symbol: &str,
        side: &str,
        price: i64,
        quantity: u64,
        tif: &str,
    ) -> PyResult<PySubmitResult> {
        let sym = parse_symbol(symbol)?;
        let side = parse_side(side)?;
        let tif = parse_tif(tif)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(ex.submit_limit(side, Price(price), quantity, tif).into())
    }

    fn submit_market(
        &mut self,
        symbol: &str,
        side: &str,
        quantity: u64,
    ) -> PyResult<PySubmitResult> {
        let sym = parse_symbol(symbol)?;
        let side = parse_side(side)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(ex.submit_market(side, quantity).into())
    }

    fn cancel(&mut self, symbol: &str, order_id: u64) -> PyResult<PyCancelResult> {
        let sym = parse_symbol(symbol)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(ex.cancel(OrderId(order_id)).into())
    }

    fn modify(
        &mut self,
        symbol: &str,
        order_id: u64,
        new_price: i64,
        new_quantity: u64,
    ) -> PyResult<PyModifyResult> {
        let sym = parse_symbol(symbol)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(ex
            .modify(OrderId(order_id), Price(new_price), new_quantity)
            .into())
    }

    /// Number of symbols.
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!("MultiExchange(symbols={})", self.inner.len())
    }
}
