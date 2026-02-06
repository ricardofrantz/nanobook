use crate::types::side_str;
use nanobook::Order;
use pyo3::prelude::*;

#[pyclass(name = "Order")]
#[derive(Clone)]
pub struct PyOrder {
    pub inner: Order,
}

#[pymethods]
impl PyOrder {
    #[getter]
    fn id(&self) -> u64 {
        self.inner.id.0
    }

    #[getter]
    fn side(&self) -> &str {
        side_str(self.inner.side)
    }

    #[getter]
    fn price(&self) -> i64 {
        self.inner.price.0
    }

    #[getter]
    fn original_quantity(&self) -> u64 {
        self.inner.original_quantity
    }

    #[getter]
    fn remaining_quantity(&self) -> u64 {
        self.inner.remaining_quantity
    }

    #[getter]
    fn filled_quantity(&self) -> u64 {
        self.inner.filled_quantity
    }

    #[getter]
    fn status(&self) -> String {
        format!("{:?}", self.inner.status).to_lowercase()
    }

    #[getter]
    fn time_in_force(&self) -> String {
        format!("{:?}", self.inner.time_in_force).to_lowercase()
    }

    #[getter]
    fn timestamp(&self) -> u64 {
        self.inner.timestamp
    }

    fn __repr__(&self) -> String {
        format!(
            "Order(id={}, side={}, price={}, qty={}/{}, status={})",
            self.inner.id.0,
            side_str(self.inner.side),
            self.inner.price.0,
            self.inner.filled_quantity,
            self.inner.original_quantity,
            format!("{:?}", self.inner.status).to_lowercase()
        )
    }
}
