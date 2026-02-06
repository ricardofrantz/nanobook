use nanobook::MultiExchange;
use pyo3::prelude::*;

use crate::exchange::PyExchange;
use crate::types::parse_symbol;

/// Multi-symbol exchange wrapping one Exchange per symbol.
///
/// Example::
///
///     multi = MultiExchange()
///     aapl = multi.get_or_create("AAPL")
///     aapl.submit_limit("buy", 15000, 100, "gtc")
///
#[pyclass(name = "MultiExchange")]
pub struct PyMultiExchange {
    inner: MultiExchange,
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
    /// Use this for read-only queries or one-shot setups. For ongoing work,
    /// create standalone ``Exchange`` instances and manage them yourself.
    fn get_or_create(&mut self, symbol: &str) -> PyResult<PyExchange> {
        let sym = parse_symbol(symbol)?;
        let ex = self.inner.get_or_create(&sym);
        Ok(PyExchange::from_exchange(ex.clone()))
    }

    /// List all symbols that have exchanges.
    fn symbols(&self) -> Vec<String> {
        self.inner.symbols().map(|s| s.as_str().to_string()).collect()
    }

    /// Number of symbols.
    fn len(&self) -> usize {
        self.inner.len()
    }

    fn __repr__(&self) -> String {
        format!("MultiExchange(symbols={})", self.inner.len())
    }
}
