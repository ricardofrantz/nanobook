//! Fast backtest bridge: simulate portfolio returns from a pre-computed weight schedule.
//!
//! Python computes the weight schedule (factor models, signals, etc.),
//! Rust handles the inner simulation loop (rebalance, track positions, compute returns).

use crate::portfolio::metrics::{Metrics, compute_metrics};
use crate::portfolio::{CostModel, Portfolio};
use crate::types::Symbol;

/// Result of a backtest simulation.
#[derive(Clone, Debug)]
pub struct BacktestBridgeResult {
    /// Per-period returns.
    pub returns: Vec<f64>,
    /// Equity curve (one entry per date).
    pub equity_curve: Vec<i64>,
    /// Final portfolio state.
    pub final_cash: i64,
    /// Computed metrics (None if no returns).
    pub metrics: Option<Metrics>,
}

/// Simulate portfolio returns from a pre-computed weight schedule.
///
/// # Arguments
///
/// * `weight_schedule` — Ordered list of (date_index, weights) where weights is `(Symbol, f64)`.
///   Each entry triggers a rebalance to the given weights at the given date.
/// * `price_schedule` — Per-date prices: each entry is a list of `(Symbol, price_cents)` for
///   all symbols on that date. Must be parallel with `weight_schedule` (same length, same order).
/// * `initial_cash_cents` — Starting cash (e.g., `1_000_000_00` = $1M).
/// * `cost_bps` — Transaction cost in basis points (e.g., 15 = 0.15%).
/// * `periods_per_year` — Annualization factor (252 for daily, 12 for monthly).
/// * `risk_free` — Risk-free rate per period.
///
/// Returns an empty result (no returns, no metrics) for invalid inputs:
/// mismatched schedule lengths, non-positive cash, NaN/Inf weights,
/// negative prices, or cost > 100%.
pub fn backtest_weights(
    weight_schedule: &[Vec<(Symbol, f64)>],
    price_schedule: &[Vec<(Symbol, i64)>],
    initial_cash_cents: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
) -> BacktestBridgeResult {
    // Input validation: mismatched schedule lengths
    if weight_schedule.len() != price_schedule.len() {
        return empty_result(initial_cash_cents);
    }
    // Non-positive initial cash
    if initial_cash_cents <= 0 {
        return empty_result(initial_cash_cents);
    }
    // Nonsensical cost (>100%)
    if cost_bps > 10_000 {
        return empty_result(initial_cash_cents);
    }
    // NaN/Inf weights or negative prices
    for (weights, prices) in weight_schedule.iter().zip(price_schedule.iter()) {
        for &(_, w) in weights {
            if !w.is_finite() {
                return empty_result(initial_cash_cents);
            }
        }
        for &(_, p) in prices {
            if p < 0 {
                return empty_result(initial_cash_cents);
            }
        }
    }

    let cost_model = CostModel {
        commission_bps: cost_bps,
        slippage_bps: 0,
        min_trade_fee: 0,
    };
    let mut portfolio = Portfolio::new(initial_cash_cents, cost_model);
    let mut equity_curve = Vec::with_capacity(weight_schedule.len() + 1);
    equity_curve.push(initial_cash_cents);

    for (weights, prices) in weight_schedule.iter().zip(price_schedule.iter()) {
        // Rebalance to target weights
        portfolio.rebalance_simple(weights, prices);

        // Record return for this period
        portfolio.record_return(prices);

        // Track equity
        let equity = portfolio.total_equity(prices);
        equity_curve.push(equity);
    }

    let returns = portfolio.returns().to_vec();
    let metrics = compute_metrics(&returns, periods_per_year, risk_free);

    BacktestBridgeResult {
        returns,
        equity_curve,
        final_cash: portfolio.cash(),
        metrics,
    }
}

fn empty_result(initial_cash_cents: i64) -> BacktestBridgeResult {
    BacktestBridgeResult {
        returns: Vec::new(),
        equity_curve: vec![initial_cash_cents],
        final_cash: initial_cash_cents,
        metrics: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }
    fn msft() -> Symbol {
        Symbol::new("MSFT")
    }

    #[test]
    fn basic_two_period_backtest() {
        let weights = vec![
            vec![(aapl(), 0.5), (msft(), 0.5)],
            vec![(aapl(), 0.3), (msft(), 0.7)],
        ];
        let prices = vec![
            vec![(aapl(), 150_00), (msft(), 300_00)],
            vec![(aapl(), 155_00), (msft(), 310_00)],
        ];

        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);

        assert_eq!(result.returns.len(), 2);
        assert_eq!(result.equity_curve.len(), 3); // initial + 2 periods
        assert!(result.metrics.is_some());
    }

    #[test]
    fn zero_cost_preserves_equity() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), 100_00)]];

        let result = backtest_weights(&weights, &prices, 1_000_000_00, 0, 252.0, 0.0);

        // With zero cost and no price movement, equity should be ~initial
        let final_eq = *result.equity_curve.last().unwrap();
        assert!((final_eq - 1_000_000_00).abs() < 200_00); // rounding tolerance
    }

    #[test]
    fn empty_schedule() {
        let result = backtest_weights(&[], &[], 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
        assert!(result.metrics.is_none());
        assert_eq!(result.equity_curve.len(), 1);
    }
}
