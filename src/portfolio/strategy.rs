//! Strategy trait and backtest runner.
//!
//! Provides a batch-oriented backtesting framework. Users implement
//! `compute_weights` to generate target allocations; the framework
//! handles rebalancing, return recording, and metrics computation.
//!
//! # Example
//!
//! ```ignore
//! use nanobook::portfolio::strategy::{Strategy, run_backtest, BacktestResult};
//! use nanobook::portfolio::CostModel;
//! use nanobook::Symbol;
//!
//! struct EqualWeight;
//!
//! impl Strategy for EqualWeight {
//!     fn compute_weights(
//!         &self,
//!         _bar_index: usize,
//!         prices: &[(Symbol, i64)],
//!         _portfolio: &nanobook::portfolio::Portfolio,
//!     ) -> Vec<(Symbol, f64)> {
//!         let n = prices.len() as f64;
//!         prices.iter().map(|&(sym, _)| (sym, 1.0 / n)).collect()
//!     }
//! }
//! ```

use crate::portfolio::{CostModel, Metrics, Portfolio};
use crate::types::Symbol;

/// A trading strategy that produces target portfolio weights each period.
///
/// Strategies are batch-oriented: given a bar index, current prices, and
/// portfolio state, they return target weights. The backtest runner handles
/// rebalancing and return tracking.
pub trait Strategy {
    /// Compute target portfolio weights for the given bar.
    ///
    /// Returns `(symbol, weight)` pairs. Weights should sum to ≤ 1.0.
    /// Symbols not in the returned vec will be closed.
    fn compute_weights(
        &self,
        bar_index: usize,
        prices: &[(Symbol, i64)],
        portfolio: &Portfolio,
    ) -> Vec<(Symbol, f64)>;
}

/// Result of a backtest run.
#[derive(Clone, Debug)]
pub struct BacktestResult {
    /// The final portfolio state (positions, cash, equity curve, returns).
    pub portfolio: Portfolio,
    /// Computed performance metrics (None if no returns recorded).
    pub metrics: Option<Metrics>,
}

/// Run a backtest of a strategy over a price series.
///
/// Each element of `price_series` is one bar's prices: `[(symbol, price)]`.
/// The strategy is called each bar to produce weights, and the portfolio
/// is rebalanced via simple fill (instant execution at bar prices).
///
/// # Arguments
///
/// * `strategy` — The strategy to backtest
/// * `price_series` — Slice of bars, each bar is `&[(Symbol, i64)]`
/// * `initial_cash` — Starting cash in cents
/// * `cost_model` — Transaction cost model
/// * `periods_per_year` — For annualizing metrics (12 for monthly, 252 for daily)
/// * `risk_free` — Risk-free rate per period
pub fn run_backtest<S: Strategy>(
    strategy: &S,
    price_series: &[Vec<(Symbol, i64)>],
    initial_cash: i64,
    cost_model: CostModel,
    periods_per_year: f64,
    risk_free: f64,
) -> BacktestResult {
    let mut portfolio = Portfolio::new(initial_cash, cost_model);

    for (i, prices) in price_series.iter().enumerate() {
        let weights = strategy.compute_weights(i, prices, &portfolio);
        portfolio.rebalance_simple(&weights, prices);
        portfolio.record_return(prices);
    }

    let metrics =
        crate::portfolio::compute_metrics(portfolio.returns(), periods_per_year, risk_free);

    BacktestResult { portfolio, metrics }
}

/// Equal-weight strategy: allocates equally across all symbols.
pub struct EqualWeight;

impl Strategy for EqualWeight {
    fn compute_weights(
        &self,
        _bar_index: usize,
        prices: &[(Symbol, i64)],
        _portfolio: &Portfolio,
    ) -> Vec<(Symbol, f64)> {
        if prices.is_empty() {
            return Vec::new();
        }
        let n = prices.len() as f64;
        prices.iter().map(|&(sym, _)| (sym, 1.0 / n)).collect()
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::inconsistent_digit_grouping)]

    use super::*;

    fn sym(s: &str) -> Symbol {
        Symbol::new(s)
    }

    #[test]
    fn equal_weight_single_stock() {
        let prices = vec![
            vec![(sym("AAPL"), 150_00)],
            vec![(sym("AAPL"), 155_00)],
            vec![(sym("AAPL"), 160_00)],
        ];

        let result = run_backtest(
            &EqualWeight,
            &prices,
            1_000_000_00,
            CostModel::zero(),
            12.0,
            0.0,
        );

        assert!(result.portfolio.returns().len() == 3);
        assert!(result.metrics.is_some());
        let m = result.metrics.unwrap();
        assert!(m.total_return > 0.0); // prices went up
    }

    #[test]
    fn equal_weight_two_stocks() {
        let prices = vec![
            vec![(sym("AAPL"), 150_00), (sym("MSFT"), 300_00)],
            vec![(sym("AAPL"), 155_00), (sym("MSFT"), 310_00)],
            vec![(sym("AAPL"), 145_00), (sym("MSFT"), 320_00)],
        ];

        let result = run_backtest(
            &EqualWeight,
            &prices,
            1_000_000_00,
            CostModel::zero(),
            12.0,
            0.0,
        );

        assert_eq!(result.portfolio.returns().len(), 3);
        assert!(result.metrics.is_some());
    }

    #[test]
    fn empty_price_series() {
        let prices: Vec<Vec<(Symbol, i64)>> = vec![];
        let result = run_backtest(
            &EqualWeight,
            &prices,
            1_000_000_00,
            CostModel::zero(),
            12.0,
            0.0,
        );

        assert!(result.portfolio.returns().is_empty());
        assert!(result.metrics.is_none());
    }

    #[test]
    fn custom_strategy() {
        // Strategy that only buys when bar_index > 0
        struct DelayedBuy;
        impl Strategy for DelayedBuy {
            fn compute_weights(
                &self,
                bar_index: usize,
                prices: &[(Symbol, i64)],
                _portfolio: &Portfolio,
            ) -> Vec<(Symbol, f64)> {
                if bar_index == 0 {
                    Vec::new() // Cash only on first bar
                } else {
                    prices.iter().map(|&(sym, _)| (sym, 1.0)).collect()
                }
            }
        }

        let prices = vec![
            vec![(sym("AAPL"), 100_00)],
            vec![(sym("AAPL"), 110_00)],
            vec![(sym("AAPL"), 120_00)],
        ];

        let result = run_backtest(
            &DelayedBuy,
            &prices,
            100_000_00,
            CostModel::zero(),
            12.0,
            0.0,
        );

        // First bar: no position, return ≈ 0
        // Second bar: bought at 110, position exists
        assert_eq!(result.portfolio.returns().len(), 3);
    }

    #[test]
    fn backtest_with_costs() {
        let cost_model = CostModel {
            commission_bps: 10,
            slippage_bps: 5,
            min_trade_fee: 0,
        };

        let prices = vec![
            vec![(sym("AAPL"), 150_00)],
            vec![(sym("AAPL"), 150_00)], // Same price
            vec![(sym("AAPL"), 150_00)],
        ];

        let result = run_backtest(&EqualWeight, &prices, 1_000_000_00, cost_model, 12.0, 0.0);

        // With constant prices and costs, returns should be slightly negative
        let m = result.metrics.unwrap();
        assert!(m.total_return < 0.0);
    }

    #[test]
    fn equal_weight_empty_bar() {
        let strat = EqualWeight;
        let weights = strat.compute_weights(0, &[], &Portfolio::new(100_00, CostModel::zero()));
        assert!(weights.is_empty());
    }
}
