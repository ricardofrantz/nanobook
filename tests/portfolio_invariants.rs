//! Portfolio invariant tests: equity conservation, PnL correctness, metrics edge cases.

#![cfg(feature = "portfolio")]
#![allow(clippy::inconsistent_digit_grouping)]

use nanobook::Symbol;
use nanobook::portfolio::{CostModel, Portfolio, Position, compute_metrics};

fn aapl() -> Symbol {
    Symbol::new("AAPL")
}
fn msft() -> Symbol {
    Symbol::new("MSFT")
}

// === Equity Conservation ===

#[test]
fn equity_conserved_simple_rebalance() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00), (msft(), 300_00)];
    let targets = [(aapl(), 0.6), (msft(), 0.4)];

    let before = portfolio.total_equity(&prices);
    portfolio.rebalance_simple(&targets, &prices);
    let after = portfolio.total_equity(&prices);

    // With zero costs, equity should be conserved up to integer rounding
    // Max rounding error: 1 share * price per position
    let max_err = 2 * 300_00; // 2 positions, max price $300
    assert!(
        (after - before).abs() < max_err,
        "equity not conserved: before={before}, after={after}, diff={}",
        (after - before).abs()
    );
}

#[test]
fn equity_decreases_with_costs() {
    let model = CostModel {
        commission_bps: 10,
        slippage_bps: 5,
        min_trade_fee: 0,
    };
    let mut portfolio = Portfolio::new(1_000_000_00, model);
    let prices = [(aapl(), 150_00)];
    let targets = [(aapl(), 1.0)];

    let before = portfolio.total_equity(&prices);
    portfolio.rebalance_simple(&targets, &prices);
    let after = portfolio.total_equity(&prices);

    assert!(after < before, "equity should decrease with costs");
}

#[test]
fn cash_plus_positions_equals_equity() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00), (msft(), 300_00)];
    let targets = [(aapl(), 0.5), (msft(), 0.3)];

    portfolio.rebalance_simple(&targets, &prices);

    let cash = portfolio.cash();
    let pos_value: i64 = portfolio
        .positions()
        .map(|(sym, pos)| {
            let price = prices.iter().find(|(s, _)| s == sym).unwrap().1;
            pos.market_value(price)
        })
        .sum();

    let equity = portfolio.total_equity(&prices);
    assert_eq!(cash + pos_value, equity);
}

// === PnL Round-Trips ===

#[test]
fn position_pnl_round_trip() {
    let mut pos = Position::new(aapl());

    // Buy 100 @ $50, sell 100 @ $60 → $1,000 profit
    pos.apply_fill(100, 50_00);
    pos.apply_fill(-100, 60_00);

    assert!(pos.is_flat());
    assert_eq!(pos.realized_pnl, 100 * 10_00);
    assert_eq!(pos.unrealized_pnl(999_99), 0); // flat → no unrealized
}

#[test]
fn position_pnl_partial_close() {
    let mut pos = Position::new(aapl());

    pos.apply_fill(200, 50_00); // buy 200 @ $50
    pos.apply_fill(-100, 60_00); // sell 100 @ $60
    pos.apply_fill(-100, 55_00); // sell 100 @ $55

    assert!(pos.is_flat());
    assert_eq!(
        pos.realized_pnl,
        100 * 10_00 + 100 * 5_00 // $1,000 + $500 = $1,500
    );
}

#[test]
fn position_short_pnl() {
    let mut pos = Position::new(aapl());

    pos.apply_fill(-100, 60_00); // short 100 @ $60
    pos.apply_fill(100, 50_00); // cover @ $50 → $10/share profit

    assert!(pos.is_flat());
    assert_eq!(pos.realized_pnl, 100 * 10_00);
}

#[test]
fn position_flip_tracks_pnl() {
    let mut pos = Position::new(aapl());

    pos.apply_fill(100, 50_00); // long 100 @ $50
    pos.apply_fill(-200, 60_00); // sell 200: close 100 (profit), open short 100

    assert_eq!(pos.quantity, -100);
    assert_eq!(pos.avg_entry_price, 60_00);
    assert_eq!(pos.realized_pnl, 100 * 10_00); // profit on closed long
}

// === Metrics Edge Cases ===

#[test]
fn metrics_all_positive_returns() {
    let returns = vec![
        0.01, 0.02, 0.015, 0.005, 0.01, 0.02, 0.01, 0.005, 0.01, 0.02,
    ];
    let m = compute_metrics(&returns, 252.0, 0.0).unwrap();

    assert!(m.total_return > 0.0);
    assert!(m.cagr > 0.0);
    assert!(m.sharpe > 0.0);
    assert_eq!(m.max_drawdown, 0.0); // never draws down
    assert_eq!(m.winning_periods, 10);
    assert_eq!(m.losing_periods, 0);
}

#[test]
fn metrics_all_negative_returns() {
    let returns = vec![-0.01, -0.02, -0.015, -0.005, -0.01];
    let m = compute_metrics(&returns, 252.0, 0.0).unwrap();

    assert!(m.total_return < 0.0);
    assert!(m.sharpe < 0.0);
    assert!(m.max_drawdown > 0.0);
    assert_eq!(m.winning_periods, 0);
    assert_eq!(m.losing_periods, 5);
}

#[test]
fn metrics_single_period() {
    let m = compute_metrics(&[0.05], 12.0, 0.0).unwrap();
    assert!((m.total_return - 0.05).abs() < 1e-10);
    assert_eq!(m.num_periods, 1);
}

#[test]
fn metrics_zero_returns() {
    let returns = vec![0.0, 0.0, 0.0];
    let m = compute_metrics(&returns, 252.0, 0.0).unwrap();

    assert!((m.total_return).abs() < 1e-10);
    assert_eq!(m.sharpe, 0.0); // zero vol → zero sharpe
    assert_eq!(m.max_drawdown, 0.0);
}

#[test]
fn metrics_risk_free_rate() {
    let returns = vec![0.01; 12];

    let m_zero_rf = compute_metrics(&returns, 12.0, 0.0).unwrap();
    let m_high_rf = compute_metrics(&returns, 12.0, 0.005).unwrap();

    // Higher risk-free rate → lower Sharpe
    assert!(m_high_rf.sharpe < m_zero_rf.sharpe);
}

// === Cost Model ===

#[test]
fn cost_model_non_negative() {
    let model = CostModel {
        commission_bps: 100,
        slippage_bps: 50,
        min_trade_fee: 5_00,
    };

    for notional in &[0, 100, 1_000, 1_000_000, -500_000] {
        assert!(
            model.compute_cost(*notional) >= 0,
            "cost should be non-negative for notional={notional}"
        );
    }
}

#[test]
fn cost_model_min_fee_floor() {
    let model = CostModel {
        commission_bps: 1,
        slippage_bps: 0,
        min_trade_fee: 10_00, // $10 minimum
    };

    // Small trade: bps cost < min fee → min fee wins
    let cost = model.compute_cost(1_000); // $10 notional, 0.01% = $0.001
    assert_eq!(cost, 10_00);

    // Large trade: bps cost > min fee → bps cost wins
    let cost = model.compute_cost(100_000_000); // $1M, 1 bps = $100
    assert!(cost > 10_00);
}

// === Portfolio Rebalancing ===

#[test]
fn rebalance_to_zero_flattens_positions() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00)];

    // Buy
    portfolio.rebalance_simple(&[(aapl(), 1.0)], &prices);
    assert!(!portfolio.position(&aapl()).unwrap().is_flat());

    // Flatten
    portfolio.rebalance_simple(&[], &prices);
    assert!(portfolio.position(&aapl()).unwrap().is_flat());
}

#[test]
fn rebalance_multiple_rounds_stable() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00), (msft(), 300_00)];
    let targets = [(aapl(), 0.6), (msft(), 0.4)];

    // Rebalance twice with same targets and prices → should be stable
    portfolio.rebalance_simple(&targets, &prices);
    let equity_1 = portfolio.total_equity(&prices);

    portfolio.rebalance_simple(&targets, &prices);
    let equity_2 = portfolio.total_equity(&prices);

    // Second rebalance should be a near-noop
    assert!(
        (equity_2 - equity_1).abs() < 300_00,
        "second rebalance changed equity significantly"
    );
}

#[test]
fn return_series_length_matches_periods() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00)];

    for _ in 0..10 {
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);
        portfolio.record_return(&prices);
    }

    assert_eq!(portfolio.returns().len(), 10);
    assert_eq!(portfolio.equity_curve().len(), 11); // initial + 10 records
}

// === Proptest-style invariants (manual) ===

#[test]
fn weights_sum_le_one() {
    let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
    let prices = [(aapl(), 150_00), (msft(), 300_00)];
    let targets = [(aapl(), 0.4), (msft(), 0.3)];

    portfolio.rebalance_simple(&targets, &prices);

    let weights = portfolio.current_weights(&prices);
    let sum: f64 = weights.iter().map(|(_, w)| w).sum();
    assert!(
        sum <= 1.0 + 0.01, // small tolerance for rounding
        "weights sum {sum} exceeds 1.0"
    );
}

#[test]
fn zero_initial_cash_does_nothing() {
    let mut portfolio = Portfolio::new(0, CostModel::zero());
    let prices = [(aapl(), 150_00)];

    portfolio.rebalance_simple(&[(aapl(), 1.0)], &prices);
    assert_eq!(portfolio.total_equity(&prices), 0);
}
