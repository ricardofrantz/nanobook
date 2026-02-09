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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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
                assert!(
                    first_test > last_train,
                    "test must come after training"
                );
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
