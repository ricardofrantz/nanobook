//! Parallel parameter sweep over strategy configurations.

use super::metrics::{compute_metrics, Metrics};

/// Run a parameter sweep in parallel, computing metrics for each configuration.
///
/// Each invocation of `run_fn` receives a parameter set and returns a vector
/// of periodic returns. The sweep computes `Metrics` for each.
///
/// # Arguments
///
/// * `params` — Slice of parameter configurations to sweep over
/// * `periods_per_year` — Annualization factor (252 for daily, 12 for monthly)
/// * `risk_free` — Risk-free rate per period
/// * `run_fn` — Function that runs a strategy with the given params, returning returns
///
/// # Example
///
/// ```ignore
/// use nanobook::portfolio::sweep;
///
/// let params = vec![0.5_f64, 1.0, 1.5, 2.0]; // e.g., leverage levels
/// let results = sweep(&params, 12.0, 0.0, |&leverage| {
///     // Run strategy, return monthly returns
///     vec![0.01 * leverage, -0.005 * leverage, 0.02 * leverage]
/// });
/// ```
#[cfg(feature = "parallel")]
pub fn sweep<F, P>(params: &[P], periods_per_year: f64, risk_free: f64, run_fn: F) -> Vec<Option<Metrics>>
where
    F: Fn(&P) -> Vec<f64> + Sync,
    P: Sync,
{
    use rayon::prelude::*;

    params
        .par_iter()
        .map(|p| {
            let returns = run_fn(p);
            compute_metrics(&returns, periods_per_year, risk_free)
        })
        .collect()
}

#[cfg(test)]
#[cfg(feature = "parallel")]
mod tests {
    use super::*;

    #[test]
    fn sweep_basic() {
        let params = vec![1.0_f64, 2.0, 3.0];
        let results = sweep(&params, 12.0, 0.0, |&scale| {
            vec![0.01 * scale, -0.005 * scale, 0.02 * scale]
        });

        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.is_some());
        }

        // Higher scale → higher total return
        let r1 = results[0].as_ref().unwrap().total_return;
        let r2 = results[1].as_ref().unwrap().total_return;
        let r3 = results[2].as_ref().unwrap().total_return;
        assert!(r2 > r1);
        assert!(r3 > r2);
    }

    #[test]
    fn sweep_empty_params() {
        let params: Vec<f64> = vec![];
        let results = sweep(&params, 12.0, 0.0, |_: &f64| vec![0.01]);
        assert!(results.is_empty());
    }
}
