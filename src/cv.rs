//! Cross-validation splitting strategies for time series.
//!
//! Provides expanding-window time series splits, replacing
//! `sklearn.model_selection.TimeSeriesSplit`.
//!
//! # References
//!
//! - scikit-learn source: `sklearn/model_selection/_split.py`
//!   <https://github.com/scikit-learn/scikit-learn/blob/main/sklearn/model_selection/_split.py>

/// Expanding-window time series cross-validation splits.
///
/// Matches sklearn's `TimeSeriesSplit` behavior:
/// - `test_size = n_samples / (n_splits + 1)` (integer floor division).
/// - Each fold expands the training window by `test_size`.
/// - Returns `Vec<(Vec<usize>, Vec<usize>)>` — `(train_indices, test_indices)` per fold.
///
/// # Arguments
///
/// * `n_samples` — Total number of observations.
/// * `n_splits` — Number of folds.
///
/// # Returns
///
/// Vector of `(train_indices, test_indices)` tuples. May return fewer than
/// `n_splits` folds if `n_samples` is too small.
///
/// # Example
///
/// ```
/// use nanobook::cv::time_series_split;
///
/// let splits = time_series_split(10, 3);
/// assert_eq!(splits.len(), 3);
///
/// // Fold 0: train=[0..4], test=[4,5]
/// // Fold 1: train=[0..6], test=[6,7]
/// // Fold 2: train=[0..8], test=[8,9]
/// assert_eq!(splits[0].0, vec![0, 1, 2, 3]);
/// assert_eq!(splits[0].1, vec![4, 5]);
/// ```
#[cfg(feature = "portfolio")]
use crate::portfolio::metrics::{Metrics, compute_metrics};

pub fn time_series_split(n_samples: usize, n_splits: usize) -> Vec<(Vec<usize>, Vec<usize>)> {
    if n_splits < 2 || n_samples < 2 {
        return vec![];
    }

    let test_size = n_samples / (n_splits + 1);
    if test_size == 0 {
        return vec![];
    }

    // Match sklearn: test_starts = range(n - n_splits*test_size, n, test_size)
    let first_test_start = n_samples - n_splits * test_size;
    let mut splits = Vec::with_capacity(n_splits);

    for i in 0..n_splits {
        let test_start = first_test_start + i * test_size;
        let test_end = test_start + test_size;

        let train: Vec<usize> = (0..test_start).collect();
        let test: Vec<usize> = (test_start..test_end).collect();
        splits.push((train, test));
    }

    splits
}

/// One walk-forward train/test window with in-sample and out-of-sample metrics.
#[cfg(feature = "portfolio")]
#[derive(Debug, Clone)]
pub struct WalkForwardWindow {
    pub train_start: usize,
    pub train_end: usize,
    pub test_start: usize,
    pub test_end: usize,
    pub train_metrics: Option<Metrics>,
    pub test_metrics: Option<Metrics>,
}

/// Window-based rolling walk-forward analysis over a return series.
///
/// Splits `returns` into `n_windows` contiguous windows. Each window is split
/// into in-sample and out-of-sample segments by `train_pct`; metrics are computed
/// independently for each side, so OOS metrics never include train observations.
#[cfg(feature = "portfolio")]
pub fn walkforward(
    returns: &[f64],
    n_windows: usize,
    train_pct: f64,
    periods_per_year: f64,
    risk_free: f64,
) -> Vec<WalkForwardWindow> {
    if returns.is_empty()
        || n_windows == 0
        || !(0.0..1.0).contains(&train_pct)
        || !train_pct.is_finite()
    {
        return Vec::new();
    }

    let window_size = returns.len() / n_windows;
    if window_size < 2 {
        return Vec::new();
    }

    let mut windows = Vec::with_capacity(n_windows);
    for window_index in 0..n_windows {
        let start = window_index * window_size;
        let end = if window_index + 1 == n_windows {
            returns.len()
        } else {
            start + window_size
        };
        if end <= start + 1 {
            continue;
        }

        let len = end - start;
        let train_len = ((len as f64) * train_pct).floor() as usize;
        if train_len == 0 || train_len >= len {
            continue;
        }

        let train_end = start + train_len;
        windows.push(WalkForwardWindow {
            train_start: start,
            train_end,
            test_start: train_end,
            test_end: end,
            train_metrics: compute_metrics(&returns[start..train_end], periods_per_year, risk_free),
            test_metrics: compute_metrics(&returns[train_end..end], periods_per_year, risk_free),
        });
    }

    windows
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "portfolio")]
    #[test]
    fn walkforward_splits_train_and_oos_without_leakage() {
        let returns = vec![0.01; 12];
        let windows = walkforward(&returns, 3, 0.5, 252.0, 0.0);
        assert_eq!(windows.len(), 3);
        assert_eq!((windows[0].train_start, windows[0].train_end), (0, 2));
        assert_eq!((windows[0].test_start, windows[0].test_end), (2, 4));
        assert_eq!((windows[1].train_start, windows[1].train_end), (4, 6));
        assert_eq!((windows[1].test_start, windows[1].test_end), (6, 8));
        assert!(windows.iter().all(|w| w.train_end == w.test_start));
        assert!(
            windows
                .iter()
                .all(|w| w.train_metrics.is_some() && w.test_metrics.is_some())
        );
    }

    #[cfg(feature = "portfolio")]
    #[test]
    fn walkforward_rejects_invalid_edges() {
        assert!(walkforward(&[0.01, 0.02], 1, 0.0, 252.0, 0.0).is_empty());
        assert!(walkforward(&[0.01, 0.02], 1, 1.0, 252.0, 0.0).is_empty());
        assert!(walkforward(&[0.01], 4, 0.5, 252.0, 0.0).is_empty());
    }

    #[test]
    fn basic_split() {
        let splits = time_series_split(10, 3);
        assert_eq!(splits.len(), 3);

        // test_size = 10 / 4 = 2, first_test_start = 10 - 3*2 = 4
        assert_eq!(splits[0].0, vec![0, 1, 2, 3]);
        assert_eq!(splits[0].1, vec![4, 5]);

        assert_eq!(splits[1].0, vec![0, 1, 2, 3, 4, 5]);
        assert_eq!(splits[1].1, vec![6, 7]);

        assert_eq!(splits[2].0, vec![0, 1, 2, 3, 4, 5, 6, 7]);
        assert_eq!(splits[2].1, vec![8, 9]);
    }

    #[test]
    fn expanding_window() {
        let splits = time_series_split(100, 5);
        assert_eq!(splits.len(), 5);

        // Each fold's training set should be larger than the previous
        for i in 1..splits.len() {
            assert!(splits[i].0.len() > splits[i - 1].0.len());
        }

        // All test sets should be the same size
        let test_size = splits[0].1.len();
        for s in &splits {
            assert_eq!(s.1.len(), test_size);
        }
    }

    #[test]
    fn no_overlap() {
        let splits = time_series_split(50, 5);
        for (train, test) in &splits {
            // Train and test should not overlap
            for &t in test {
                assert!(!train.contains(&t), "test index {t} found in training set");
            }
            // Test should come after training
            if let (Some(&last_train), Some(&first_test)) = (train.last(), test.first()) {
                assert!(first_test > last_train, "test must come after training");
            }
        }
    }

    #[test]
    fn too_few_samples() {
        let splits = time_series_split(2, 5);
        // test_size = 2 / 6 = 0 → no splits
        assert!(splits.is_empty());
    }

    #[test]
    fn zero_splits() {
        assert!(time_series_split(100, 0).is_empty());
    }

    #[test]
    fn single_split() {
        // sklearn requires n_splits >= 2; we match that constraint
        let splits = time_series_split(10, 1);
        assert!(splits.is_empty());
    }

    #[test]
    fn large_dataset() {
        let splits = time_series_split(1000, 10);
        assert_eq!(splits.len(), 10);
        // test_size = 1000 / 11 = 90
        assert_eq!(splits[0].1.len(), 90);
    }
}
