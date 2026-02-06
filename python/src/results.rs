use pyo3::prelude::*;

use crate::types::{price_to_float, side_str};

/// Result of submitting an order.
#[pyclass(name = "SubmitResult")]
#[derive(Clone)]
pub struct PySubmitResult {
    #[pyo3(get)]
    pub order_id: u64,
    #[pyo3(get)]
    pub status: String,
    #[pyo3(get)]
    pub filled_quantity: u64,
    #[pyo3(get)]
    pub resting_quantity: u64,
    #[pyo3(get)]
    pub cancelled_quantity: u64,
    pub trades: Vec<PyTrade>,
}

#[pymethods]
impl PySubmitResult {
    #[getter]
    fn trades(&self) -> Vec<PyTrade> {
        self.trades.clone()
    }

    fn __repr__(&self) -> String {
        format!(
            "SubmitResult(order_id={}, status='{}', filled={}, resting={}, cancelled={}, trades={})",
            self.order_id,
            self.status,
            self.filled_quantity,
            self.resting_quantity,
            self.cancelled_quantity,
            self.trades.len(),
        )
    }
}

impl From<nanobook::SubmitResult> for PySubmitResult {
    fn from(r: nanobook::SubmitResult) -> Self {
        Self {
            order_id: r.order_id.0,
            status: format!("{:?}", r.status),
            filled_quantity: r.filled_quantity,
            resting_quantity: r.resting_quantity,
            cancelled_quantity: r.cancelled_quantity,
            trades: r.trades.into_iter().map(PyTrade::from).collect(),
        }
    }
}

/// Result of cancelling an order.
#[pyclass(name = "CancelResult")]
#[derive(Clone)]
pub struct PyCancelResult {
    #[pyo3(get)]
    pub success: bool,
    #[pyo3(get)]
    pub cancelled_quantity: u64,
    #[pyo3(get)]
    pub error: Option<String>,
}

#[pymethods]
impl PyCancelResult {
    fn __repr__(&self) -> String {
        if self.success {
            format!(
                "CancelResult(success=True, cancelled_quantity={})",
                self.cancelled_quantity
            )
        } else {
            format!(
                "CancelResult(success=False, error='{}')",
                self.error.as_deref().unwrap_or("unknown")
            )
        }
    }
}

impl From<nanobook::CancelResult> for PyCancelResult {
    fn from(r: nanobook::CancelResult) -> Self {
        Self {
            success: r.success,
            cancelled_quantity: r.cancelled_quantity,
            error: r.error.map(|e| format!("{e:?}")),
        }
    }
}

/// Result of modifying an order.
#[pyclass(name = "ModifyResult")]
#[derive(Clone)]
pub struct PyModifyResult {
    #[pyo3(get)]
    pub success: bool,
    #[pyo3(get)]
    pub old_order_id: u64,
    #[pyo3(get)]
    pub new_order_id: Option<u64>,
    #[pyo3(get)]
    pub cancelled_quantity: u64,
    pub trades: Vec<PyTrade>,
    #[pyo3(get)]
    pub error: Option<String>,
}

#[pymethods]
impl PyModifyResult {
    #[getter]
    fn trades(&self) -> Vec<PyTrade> {
        self.trades.clone()
    }

    fn __repr__(&self) -> String {
        if self.success {
            format!(
                "ModifyResult(success=True, old={}, new={:?}, trades={})",
                self.old_order_id,
                self.new_order_id,
                self.trades.len(),
            )
        } else {
            format!(
                "ModifyResult(success=False, error='{}')",
                self.error.as_deref().unwrap_or("unknown")
            )
        }
    }
}

impl From<nanobook::ModifyResult> for PyModifyResult {
    fn from(r: nanobook::ModifyResult) -> Self {
        Self {
            success: r.success,
            old_order_id: r.old_order_id.0,
            new_order_id: r.new_order_id.map(|id| id.0),
            cancelled_quantity: r.cancelled_quantity,
            trades: r.trades.into_iter().map(PyTrade::from).collect(),
            error: r.error.map(|e| format!("{e:?}")),
        }
    }
}

/// Result of submitting a stop order.
#[pyclass(name = "StopSubmitResult")]
#[derive(Clone)]
pub struct PyStopSubmitResult {
    #[pyo3(get)]
    pub order_id: u64,
    #[pyo3(get)]
    pub status: String,
}

#[pymethods]
impl PyStopSubmitResult {
    fn __repr__(&self) -> String {
        format!(
            "StopSubmitResult(order_id={}, status='{}')",
            self.order_id, self.status
        )
    }
}

impl From<nanobook::StopSubmitResult> for PyStopSubmitResult {
    fn from(r: nanobook::StopSubmitResult) -> Self {
        Self {
            order_id: r.order_id.0,
            status: format!("{:?}", r.status),
        }
    }
}

/// A trade that occurred in the exchange.
#[pyclass(name = "Trade")]
#[derive(Clone)]
pub struct PyTrade {
    #[pyo3(get)]
    pub trade_id: u64,
    #[pyo3(get)]
    pub price: i64,
    #[pyo3(get)]
    pub quantity: u64,
    #[pyo3(get)]
    pub aggressor_side: String,
    #[pyo3(get)]
    pub aggressor_order_id: u64,
    #[pyo3(get)]
    pub passive_order_id: u64,
    #[pyo3(get)]
    pub timestamp: u64,
}

#[pymethods]
impl PyTrade {
    /// Price as a float (dollars, not cents).
    #[getter]
    fn price_float(&self) -> f64 {
        price_to_float(nanobook::Price(self.price))
    }

    fn __repr__(&self) -> String {
        format!(
            "Trade(id={}, price=${:.2}, qty={}, side='{}')",
            self.trade_id,
            price_to_float(nanobook::Price(self.price)),
            self.quantity,
            self.aggressor_side,
        )
    }
}

impl From<nanobook::Trade> for PyTrade {
    fn from(t: nanobook::Trade) -> Self {
        Self {
            trade_id: t.id.0,
            price: t.price.0,
            quantity: t.quantity,
            aggressor_side: side_str(t.aggressor_side).to_string(),
            aggressor_order_id: t.aggressor_order_id.0,
            passive_order_id: t.passive_order_id.0,
            timestamp: t.timestamp,
        }
    }
}

/// A price level in the order book snapshot.
#[pyclass(name = "LevelSnapshot")]
#[derive(Clone)]
pub struct PyLevelSnapshot {
    #[pyo3(get)]
    pub price: i64,
    #[pyo3(get)]
    pub quantity: u64,
    #[pyo3(get)]
    pub order_count: usize,
}

#[pymethods]
impl PyLevelSnapshot {
    #[getter]
    fn price_float(&self) -> f64 {
        price_to_float(nanobook::Price(self.price))
    }

    fn __repr__(&self) -> String {
        format!(
            "Level(price=${:.2}, qty={}, orders={})",
            price_to_float(nanobook::Price(self.price)),
            self.quantity,
            self.order_count,
        )
    }
}
