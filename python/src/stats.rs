use nanobook::stats;
use pyo3::prelude::*;

/// Compute Spearman rank correlation with two-tailed p-value.
///
/// Drop-in replacement for ``scipy.stats.spearmanr(x, y)``.
/// Uses average-rank tie-breaking, matching scipy's default.
///
/// Args:
///     x: First variable (list of floats).
///     y: Second variable (list of floats, same length as x).
///
/// Returns:
///     Tuple of (correlation, p_value). Returns (NaN, NaN) if len < 3.
///
/// Example::
///
///     corr, p = nanobook.py_spearman(scores, returns)
///
#[pyfunction]
pub fn py_spearman(x: Vec<f64>, y: Vec<f64>) -> (f64, f64) {
    stats::spearman(&x, &y)
}

/// Compute quintile spread (top quintile mean - bottom quintile mean).
///
/// Sorts by ``scores``, splits into ``n_quantiles`` groups, returns the
/// difference between the top group's mean return and the bottom group's.
///
/// Args:
///     scores: Factor scores (list of floats).
///     returns: Realized returns (list of floats, same length as scores).
///     n_quantiles: Number of groups (default 5).
///
/// Returns:
///     Float: top_mean - bottom_mean. NaN if inputs are invalid.
///
/// Example::
///
///     spread = nanobook.py_quintile_spread(scores, returns, 5)
///
#[pyfunction]
#[pyo3(signature = (scores, returns, n_quantiles=5))]
pub fn py_quintile_spread(scores: Vec<f64>, returns: Vec<f64>, n_quantiles: usize) -> f64 {
    stats::quintile_spread(&scores, &returns, n_quantiles)
}

/// Compute the Deflated Sharpe Ratio.
///
/// Lopez de Prado's Deflated Sharpe Ratio adjusts an observed Sharpe ratio for
/// multiple testing and non-normal return distributions. The result is a
/// standard-normal probability after subtracting the expected maximum Sharpe
/// under the null across ``n_trials``.
///
/// Args:
///     sharpe: Observed Sharpe ratio.
///     n_trials: Number of independent strategy trials.
///     skewness: Skewness of the returns distribution.
///     kurtosis: Kurtosis of the returns distribution.
///
/// Returns:
///     Float: Deflated Sharpe Ratio probability. Returns NaN for invalid
///     inputs. For one trial, returns ``sharpe`` unchanged.
///
/// Example::
///
///     dsr = nanobook.py_deflated_sharpe(1.5, 20, 0.0, 3.0)
///
#[pyfunction]
pub fn py_deflated_sharpe(sharpe: f64, n_trials: usize, skewness: f64, kurtosis: f64) -> f64 {
    stats::deflated_sharpe(sharpe, n_trials, skewness, kurtosis)
}
