use nanobook::portfolio::Position;
use pyo3::prelude::*;

#[pyclass(name = "Position")]
#[derive(Clone)]
pub struct PyPosition {
    pub inner: Position,
}

#[pymethods]
impl PyPosition {
    #[getter]
    fn symbol(&self) -> String {
        self.inner.symbol.to_string()
    }

    #[getter]
    fn quantity(&self) -> i64 {
        self.inner.quantity
    }

    #[getter]
    fn avg_entry_price(&self) -> i64 {
        self.inner.avg_entry_price
    }

    #[getter]
    fn total_cost(&self) -> i64 {
        self.inner.total_cost
    }

    #[getter]
    fn realized_pnl(&self) -> i64 {
        self.inner.realized_pnl
    }

    fn unrealized_pnl(&self, price: i64) -> i64 {
        self.inner.unrealized_pnl(price)
    }

    fn __repr__(&self) -> String {
        format!(
            "Position(symbol={}, qty={}, avg_price={}, realized_pnl={})",
            self.inner.symbol,
            self.inner.quantity,
            self.inner.avg_entry_price,
            self.inner.realized_pnl
        )
    }
}
