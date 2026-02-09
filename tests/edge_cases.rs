// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Edge-case tests: adversarial inputs to every public API.

use nanobook::{Exchange, OrderId, Price, Side, TimeInForce};

// ============================================================================
// Empty book operations
// ============================================================================

#[test]
fn cancel_nonexistent_order() {
    let mut exchange = Exchange::new();
    let result = exchange.cancel(OrderId(999));
    assert!(!result.success);
}

#[test]
fn modify_nonexistent_order() {
    let mut exchange = Exchange::new();
    let result = exchange.modify(OrderId(999), Price(100_00), 100);
    assert!(!result.success);
}

#[test]
fn market_order_empty_book() {
    let mut exchange = Exchange::new();
    let result = exchange.submit_market(Side::Buy, 100);
    assert_eq!(result.filled_quantity, 0);
    assert!(result.trades.is_empty());
}

#[test]
fn depth_empty_book() {
    let exchange = Exchange::new();
    let snapshot = exchange.depth(100);
    assert!(snapshot.bids.is_empty());
    assert!(snapshot.asks.is_empty());
    assert!(snapshot.best_bid().is_none());
    assert!(snapshot.best_ask().is_none());
}

// ============================================================================
// Zero-quantity edge cases
// ============================================================================

#[test]
fn limit_order_zero_qty() {
    let mut exchange = Exchange::new();
    let result = exchange.submit_limit(Side::Buy, Price(100_00), 0, TimeInForce::GTC);
    // Zero-qty order should not trade or rest
    assert_eq!(result.filled_quantity, 0);
    assert_eq!(result.resting_quantity, 0);
}

#[test]
fn market_order_zero_qty() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
    let result = exchange.submit_market(Side::Buy, 0);
    assert_eq!(result.filled_quantity, 0);
}

// ============================================================================
// Price edge cases
// ============================================================================

#[test]
fn very_small_price() {
    let mut exchange = Exchange::new();
    // Price of 1 cent
    let result = exchange.submit_limit(Side::Sell, Price(1), 100, TimeInForce::GTC);
    assert_eq!(result.resting_quantity, 100);

    let result = exchange.submit_limit(Side::Buy, Price(1), 50, TimeInForce::GTC);
    assert_eq!(result.filled_quantity, 50);
}

#[test]
fn large_price() {
    let mut exchange = Exchange::new();
    let big_price = Price(1_000_000_00); // $10,000
    let result = exchange.submit_limit(Side::Sell, big_price, 1, TimeInForce::GTC);
    assert_eq!(result.resting_quantity, 1);

    let result = exchange.submit_limit(Side::Buy, big_price, 1, TimeInForce::GTC);
    assert_eq!(result.filled_quantity, 1);
}

// ============================================================================
// Symbol edge cases
// ============================================================================

#[test]
fn symbol_empty_string() {
    use nanobook::Symbol;
    let sym = Symbol::new("");
    assert_eq!(sym.as_str(), "");
}

#[test]
fn symbol_try_new_exactly_8() {
    use nanobook::Symbol;
    assert!(Symbol::try_new("12345678").is_some());
}

#[test]
fn symbol_try_new_exactly_9() {
    use nanobook::Symbol;
    assert!(Symbol::try_new("123456789").is_none());
}

// ============================================================================
// Stop order edge cases
// ============================================================================

#[test]
fn stop_on_empty_book() {
    let mut exchange = Exchange::new();
    let result = exchange.submit_stop_market(Side::Buy, Price(100_00), 100);
    // No trades yet, so stop should be pending
    assert_eq!(result.status, nanobook::StopStatus::Pending);
}

#[test]
fn cancel_already_triggered_stop() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
    // Create a trade to set last price
    exchange.submit_limit(Side::Buy, Price(100_00), 1, TimeInForce::GTC);

    // Stop that triggers immediately
    let result = exchange.submit_stop_market(Side::Buy, Price(100_00), 10);
    assert_eq!(result.status, nanobook::StopStatus::Triggered);

    // Cancel after trigger — should fail
    let cancel = exchange.cancel(result.order_id);
    assert!(!cancel.success);
}

// ============================================================================
// FOK edge cases
// ============================================================================

#[test]
fn fok_exact_fill() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

    let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);
    assert_eq!(result.filled_quantity, 100);
}

#[test]
fn fok_insufficient_liquidity() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Sell, Price(100_00), 99, TimeInForce::GTC);

    let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);
    assert_eq!(result.filled_quantity, 0);
    assert_eq!(result.cancelled_quantity, 100);
}

// ============================================================================
// IOC edge cases
// ============================================================================

#[test]
fn ioc_partial_fill() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Sell, Price(100_00), 30, TimeInForce::GTC);

    let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);
    assert_eq!(result.filled_quantity, 30);
    assert_eq!(result.cancelled_quantity, 70);
    assert_eq!(result.resting_quantity, 0);
}

// ============================================================================
// Portfolio edge cases
// ============================================================================

#[cfg(feature = "portfolio")]
mod portfolio_edges {
    use nanobook::Symbol;
    use nanobook::portfolio::{CostModel, Portfolio};

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn empty_targets_closes_all() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 100_00)];
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);
        assert!(portfolio.position(&aapl()).unwrap().quantity > 0);

        // Rebalance to empty targets — should close AAPL
        portfolio.rebalance_simple(&[], &prices);
        assert!(portfolio.position(&aapl()).unwrap().is_flat());
    }

    #[test]
    fn rebalance_with_zero_price_skips_symbol() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 0)];
        // Zero price should be skipped (no divide-by-zero)
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);
        assert!(portfolio.position(&aapl()).is_none());
    }

    #[test]
    fn record_return_with_no_positions() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        portfolio.record_return(&[]);
        assert!(portfolio.returns().is_empty() || portfolio.returns().len() == 1);
    }

    #[test]
    fn current_weights_zero_equity() {
        let portfolio = Portfolio::new(0, CostModel::zero());
        let weights = portfolio.current_weights(&[]);
        assert!(weights.is_empty());
    }
}
