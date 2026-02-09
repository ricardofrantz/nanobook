use nanobook::cv;
use pyo3::prelude::*;

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
pub fn py_time_series_split(
    n_samples: usize,
    n_splits: usize,
) -> Vec<(Vec<usize>, Vec<usize>)> {
    cv::time_series_split(n_samples, n_splits)
}
