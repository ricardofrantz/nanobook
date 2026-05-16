use nanobook::volatility::{RealizedVolMethod, realized_vol};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Estimate realized volatility from OHLC bars.
#[pyfunction]
#[pyo3(signature = (open, high, low, close, method="close_to_close"))]
pub fn py_realized_vol(
    open: Vec<f64>,
    high: Vec<f64>,
    low: Vec<f64>,
    close: Vec<f64>,
    method: &str,
) -> PyResult<f64> {
    let method = RealizedVolMethod::parse(method)
        .ok_or_else(|| PyValueError::new_err(format!("unknown realized_vol method: {method}")))?;
    Ok(realized_vol(&open, &high, &low, &close, method))
}
