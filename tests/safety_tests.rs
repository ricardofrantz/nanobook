// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Safety tests: input validation, edge cases, non-panicking behavior.

use nanobook::Symbol;

// ============================================================================
// Symbol::from_str_truncated
// ============================================================================

#[test]
fn symbol_truncated_empty() {
    let sym = Symbol::from_str_truncated("");
    assert_eq!(sym.as_str(), "");
}

#[test]
fn symbol_truncated_exact_8() {
    let sym = Symbol::from_str_truncated("12345678");
    assert_eq!(sym.as_str(), "12345678");
}

#[test]
fn symbol_truncated_9_bytes() {
    let sym = Symbol::from_str_truncated("123456789");
    assert_eq!(sym.as_str(), "12345678");
}

#[test]
fn symbol_truncated_long_string() {
    let sym = Symbol::from_str_truncated("VERYLONGSYMBOLNAME");
    assert_eq!(sym.as_str(), "VERYLONG");
}

#[test]
fn symbol_truncated_unicode_boundary() {
    // "Ω" is 2 bytes (0xCE 0xA9). If we have 7 ASCII + "Ω" = 9 bytes,
    // truncation at 8 would split the Ω. Should back up to 7.
    let sym = Symbol::from_str_truncated("1234567Ω");
    assert_eq!(sym.as_str(), "1234567");
}

#[test]
fn symbol_truncated_all_ascii_normal() {
    let sym = Symbol::from_str_truncated("AAPL");
    assert_eq!(sym.as_str(), "AAPL");
}

// ============================================================================
// Backtest bridge validation
// ============================================================================

#[cfg(feature = "portfolio")]
mod backtest {
    use nanobook::Symbol;
    use nanobook::backtest_bridge::backtest_weights;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn mismatched_schedule_lengths() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 110_00)], // extra period
        ];
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
        assert!(result.metrics.is_none());
    }

    #[test]
    fn nan_weight_returns_empty() {
        let weights = vec![vec![(aapl(), f64::NAN)]];
        let prices = vec![vec![(aapl(), 100_00)]];
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn inf_weight_returns_empty() {
        let weights = vec![vec![(aapl(), f64::INFINITY)]];
        let prices = vec![vec![(aapl(), 100_00)]];
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn negative_price_returns_empty() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), -100)]];
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn zero_initial_cash_returns_empty() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), 100_00)]];
        let result = backtest_weights(&weights, &prices, 0, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn negative_initial_cash_returns_empty() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), 100_00)]];
        let result = backtest_weights(&weights, &prices, -1, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn cost_bps_over_100_percent() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), 100_00)]];
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10_001, 252.0, 0.0);
        assert!(result.returns.is_empty());
    }

    #[test]
    fn empty_schedules_still_work() {
        let result = backtest_weights(&[], &[], 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
        assert_eq!(result.equity_curve.len(), 1);
        assert_eq!(result.final_cash, 1_000_000_00);
    }
}

// ============================================================================
// Portfolio with zero/negative equity rebalance
// ============================================================================

#[cfg(feature = "portfolio")]
mod portfolio_safety {
    use nanobook::Symbol;
    use nanobook::portfolio::{CostModel, Portfolio};

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn rebalance_simple_with_zero_equity_is_noop() {
        // Construct a portfolio and drain the cash to zero through trading
        let mut portfolio = Portfolio::new(100_00, CostModel::zero());
        let prices = [(aapl(), 100_00)];
        // Buy 1 share at $100 — cash goes to 0
        portfolio.rebalance_simple(&[(aapl(), 1.0)], &prices);

        // Now set prices to zero — equity becomes 0
        let zero_prices = [(aapl(), 0)];
        let equity = portfolio.total_equity(&zero_prices);
        assert_eq!(equity, 0);

        // Rebalancing with zero equity should be a no-op
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &zero_prices);
    }
}
