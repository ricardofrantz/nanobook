use nanobook::{Price, Side, TimeInForce};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

/// Parse a side string ("buy"/"sell") into a Side enum.
pub fn parse_side(s: &str) -> PyResult<Side> {
    match s.to_ascii_lowercase().as_str() {
        "buy" | "b" => Ok(Side::Buy),
        "sell" | "s" => Ok(Side::Sell),
        _ => Err(PyValueError::new_err(format!(
            "Invalid side '{s}'. Use 'buy' or 'sell'."
        ))),
    }
}

/// Parse a time-in-force string into a TimeInForce enum.
pub fn parse_tif(s: &str) -> PyResult<TimeInForce> {
    match s.to_ascii_lowercase().as_str() {
        "gtc" => Ok(TimeInForce::GTC),
        "ioc" => Ok(TimeInForce::IOC),
        "fok" => Ok(TimeInForce::FOK),
        _ => Err(PyValueError::new_err(format!(
            "Invalid time_in_force '{s}'. Use 'gtc', 'ioc', or 'fok'."
        ))),
    }
}

/// Format a Side as a Python string.
pub fn side_str(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

/// Format a Price as a dollars float for Python.
pub fn price_to_float(price: Price) -> f64 {
    price.0 as f64 / 100.0
}

/// Parse a symbol string, returning an error if > 8 bytes.
pub fn parse_symbol(s: &str) -> PyResult<nanobook::Symbol> {
    nanobook::Symbol::try_new(s).ok_or_else(|| {
        PyValueError::new_err(format!(
            "Symbol '{s}' exceeds 8 bytes. Use a shorter symbol."
        ))
    })
}
