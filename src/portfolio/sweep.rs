//! Parallel parameter sweep over strategy configurations.

use super::metrics::{compute_metrics, Metrics};
use super::strategy::{BacktestResult, Strategy, run_backtest};

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

/// Run a parameter sweep over strategy configurations in parallel.
///
/// For each parameter, constructs a strategy via `make_strategy` and runs
/// a full backtest. Returns `BacktestResult` for each parameter set.
///
/// # Example
///
/// ```ignore
/// use nanobook::portfolio::sweep::sweep_strategy;
///
/// let params = vec![0.5_f64, 1.0, 1.5];
/// let results = sweep_strategy(&params, &prices, initial_cash, cost_model, 12.0, 0.0, |&weight| {
///     MyStrategy { weight }
/// });
/// ```
#[cfg(feature = "parallel")]
pub fn sweep_strategy<F, P, S>(
    params: &[P],
    price_series: &[Vec<(crate::Symbol, i64)>],
    initial_cash: i64,
    cost_model: super::CostModel,
    periods_per_year: f64,
    risk_free: f64,
    make_strategy: F,
) -> Vec<BacktestResult>
where
    F: Fn(&P) -> S + Sync,
    P: Sync,
    S: Strategy,
{
    use rayon::prelude::*;

    params
        .par_iter()
        .map(|p| {
            let strategy = make_strategy(p);
            run_backtest(&strategy, price_series, initial_cash, cost_model, periods_per_year, risk_free)
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

    #[test]
    fn sweep_strategy_basic() {
        use crate::portfolio::{CostModel, EqualWeight};
        use crate::Symbol;

        fn sym(s: &str) -> Symbol {
            Symbol::new(s)
        }

        let prices = vec![
            vec![(sym("A"), 100_00)],
            vec![(sym("A"), 110_00)],
            vec![(sym("A"), 105_00)],
        ];

        // Sweep over different initial cash levels
        let params = vec![100_000_00_i64, 500_000_00, 1_000_000_00];
        let results = sweep_strategy(
            &params,
            &prices,
            1_000_000_00, // base cash (overridden by make_strategy)
            CostModel::zero(),
            12.0,
            0.0,
            |_| EqualWeight,
        );

        assert_eq!(results.len(), 3);
        for r in &results {
            assert!(r.metrics.is_some());
        }
    }
}
