use nanobook::cv;
use pyo3::prelude::*;
use pyo3::types::{PyDict, PyList};

use crate::metrics::PyMetrics;

/// Expanding-window time series cross-validation splits.
///
/// Drop-in replacement for ``sklearn.model_selection.TimeSeriesSplit``.
///
/// Args:
///     n_samples: Total number of observations.
///     n_splits: Number of folds.
///
/// Returns:
///     List of (train_indices, test_indices) tuples.
///
/// Example::
///
///     for train_idx, test_idx in nanobook.py_time_series_split(100, 5):
///         train_data = data[train_idx]
///         test_data = data[test_idx]
///
#[pyfunction]
#[pyo3(signature = (n_samples, n_splits=5))]
pub fn py_time_series_split(n_samples: usize, n_splits: usize) -> Vec<(Vec<usize>, Vec<usize>)> {
    cv::time_series_split(n_samples, n_splits)
}

/// Window-based walk-forward analysis with IS/OOS metrics per window.
#[pyfunction]
#[pyo3(signature = (returns, params=None, n_windows=5, train_pct=0.7, periods_per_year=252.0, risk_free=0.0))]
pub fn py_walkforward(
    py: Python<'_>,
    returns: Vec<f64>,
    params: Option<Vec<f64>>,
    n_windows: usize,
    train_pct: f64,
    periods_per_year: f64,
    risk_free: f64,
) -> PyResult<Py<PyAny>> {
    let windows =
        py.detach(|| cv::walkforward(&returns, n_windows, train_pct, periods_per_year, risk_free));

    let items = PyList::empty(py);
    for window in windows {
        let item = PyDict::new(py);
        item.set_item("train_start", window.train_start)?;
        item.set_item("train_end", window.train_end)?;
        item.set_item("test_start", window.test_start)?;
        item.set_item("test_end", window.test_end)?;
        item.set_item("train_metrics", window.train_metrics.map(PyMetrics::from))?;
        item.set_item("test_metrics", window.test_metrics.map(PyMetrics::from))?;
        if let Some(params) = params.as_ref() {
            item.set_item("params", params.clone())?;
        }
        items.append(item)?;
    }

    Ok(items.into_any().unbind())
}
