use nanobook::optimize;
use pyo3::prelude::*;
use pyo3::types::PyDict;

fn to_weights_dict<'py>(
    py: Python<'py>,
    symbols: &[String],
    weights: Vec<f64>,
) -> PyResult<Bound<'py, PyDict>> {
    let out = PyDict::new(py);

    if symbols.len() != weights.len() {
        return Ok(out);
    }

    for (s, w) in symbols.iter().zip(weights.iter()) {
        out.set_item(s, *w)?;
    }
    Ok(out)
}

fn sanitize_symbols(symbols: Vec<String>) -> Vec<String> {
    // Preserve order but reject empty names.
    symbols
        .into_iter()
        .filter(|s| !s.trim().is_empty())
        .collect()
}

#[pyfunction]
pub fn optimize_min_variance(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
) -> PyResult<PyObject> {
    let symbols = sanitize_symbols(symbols);
    let w = py.allow_threads(|| optimize::optimize_min_variance(&returns_matrix));
    Ok(to_weights_dict(py, &symbols, w)?.into())
}

#[pyfunction]
pub fn py_optimize_min_variance(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
) -> PyResult<PyObject> {
    optimize_min_variance(py, returns_matrix, symbols)
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, risk_free=0.0))]
pub fn optimize_max_sharpe(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    risk_free: f64,
) -> PyResult<PyObject> {
    let symbols = sanitize_symbols(symbols);
    let w = py.allow_threads(|| optimize::optimize_max_sharpe(&returns_matrix, risk_free));
    Ok(to_weights_dict(py, &symbols, w)?.into())
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, risk_free=0.0))]
pub fn py_optimize_max_sharpe(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    risk_free: f64,
) -> PyResult<PyObject> {
    optimize_max_sharpe(py, returns_matrix, symbols, risk_free)
}

#[pyfunction]
pub fn optimize_risk_parity(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
) -> PyResult<PyObject> {
    let symbols = sanitize_symbols(symbols);
    let w = py.allow_threads(|| optimize::optimize_risk_parity(&returns_matrix));
    Ok(to_weights_dict(py, &symbols, w)?.into())
}

#[pyfunction]
pub fn py_optimize_risk_parity(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
) -> PyResult<PyObject> {
    optimize_risk_parity(py, returns_matrix, symbols)
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, alpha=0.95))]
pub fn optimize_cvar(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    alpha: f64,
) -> PyResult<PyObject> {
    let symbols = sanitize_symbols(symbols);
    let w = py.allow_threads(|| optimize::optimize_cvar(&returns_matrix, alpha));
    Ok(to_weights_dict(py, &symbols, w)?.into())
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, alpha=0.95))]
pub fn py_optimize_cvar(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    alpha: f64,
) -> PyResult<PyObject> {
    optimize_cvar(py, returns_matrix, symbols, alpha)
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, alpha=0.95))]
pub fn optimize_cdar(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    alpha: f64,
) -> PyResult<PyObject> {
    let symbols = sanitize_symbols(symbols);
    let w = py.allow_threads(|| optimize::optimize_cdar(&returns_matrix, alpha));
    Ok(to_weights_dict(py, &symbols, w)?.into())
}

#[pyfunction]
#[pyo3(signature = (returns_matrix, symbols, alpha=0.95))]
pub fn py_optimize_cdar(
    py: Python<'_>,
    returns_matrix: Vec<Vec<f64>>,
    symbols: Vec<String>,
    alpha: f64,
) -> PyResult<PyObject> {
    optimize_cdar(py, returns_matrix, symbols, alpha)
}
