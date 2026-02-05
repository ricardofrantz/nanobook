// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Property-based tests for order book invariants.
//!
//! These tests use proptest to verify that key invariants hold
//! across randomly generated scenarios.

use nanobook::{Exchange, Price, Side, TimeInForce};
use proptest::prelude::*;

/// Generate a valid price (positive, reasonable range)
fn price_strategy() -> impl Strategy<Value = Price> {
    (1i64..=100_000i64).prop_map(Price)
}

/// Generate a valid quantity
fn quantity_strategy() -> impl Strategy<Value = u64> {
    1u64..=10_000u64
}

/// Generate a side
fn side_strategy() -> impl Strategy<Value = Side> {
    prop_oneof![Just(Side::Buy), Just(Side::Sell)]
}

/// Generate a time-in-force
fn tif_strategy() -> impl Strategy<Value = TimeInForce> {
    prop_oneof![
        Just(TimeInForce::GTC),
        Just(TimeInForce::IOC),
        Just(TimeInForce::FOK),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    // ========================================================================
    // CONSERVATION INVARIANTS
    // ========================================================================

    /// Quantity is conserved: filled + resting + cancelled = original
    #[test]
    fn quantity_conservation(
        price in price_strategy(),
        qty in quantity_strategy(),
        side in side_strategy(),
    ) {
        let mut exchange = Exchange::new();
        let result = exchange.submit_limit(side, price, qty, TimeInForce::GTC);

        let total = result.filled_quantity + result.resting_quantity + result.cancelled_quantity;

        prop_assert_eq!(total, qty, "quantity not conserved: filled={} + resting={} + cancelled={} != {}",
            result.filled_quantity, result.resting_quantity, result.cancelled_quantity, qty);
    }

    /// Total quantity in book equals sum of all resting orders
    #[test]
    fn book_quantity_consistency(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            1..50
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        }

        let snapshot = exchange.depth(1000);

        // Sum bid quantities from snapshot
        let bid_qty: u64 = snapshot.bids.iter().map(|l| l.quantity).sum();
        let ask_qty: u64 = snapshot.asks.iter().map(|l| l.quantity).sum();

        // Get book quantities from bids/asks
        let book_bid_qty: u64 = exchange.book().bids().total_quantity();
        let book_ask_qty: u64 = exchange.book().asks().total_quantity();

        prop_assert_eq!(bid_qty, book_bid_qty, "bid quantity mismatch in snapshot");
        prop_assert_eq!(ask_qty, book_ask_qty, "ask quantity mismatch in snapshot");
    }

    // ========================================================================
    // PRICE INVARIANTS
    // ========================================================================

    /// Trades only execute at prices within limit
    #[test]
    fn trades_within_price_limit(
        resting_price in price_strategy(),
        resting_qty in quantity_strategy(),
        incoming_price in price_strategy(),
        incoming_qty in quantity_strategy(),
    ) {
        let mut exchange = Exchange::new();

        // Add a resting ask
        exchange.submit_limit(Side::Sell, resting_price, resting_qty, TimeInForce::GTC);

        // Submit incoming buy
        let result = exchange.submit_limit(Side::Buy, incoming_price, incoming_qty, TimeInForce::GTC);

        for trade in &result.trades {
            // Buy trades should execute at or below the buyer's limit
            prop_assert!(
                trade.price <= incoming_price,
                "buy executed above limit: trade {} > limit {}",
                trade.price.0, incoming_price.0
            );
        }
    }

    /// Best bid is always less than best ask (no crossed book)
    #[test]
    fn no_crossed_book(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            1..100
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        }

        let (best_bid, best_ask) = exchange.best_bid_ask();

        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            prop_assert!(
                bid < ask,
                "crossed book: bid {} >= ask {}",
                bid.0, ask.0
            );
        }
    }

    // ========================================================================
    // TIME-IN-FORCE INVARIANTS
    // ========================================================================

    /// IOC orders never rest on the book
    #[test]
    fn ioc_never_rests(
        price in price_strategy(),
        qty in quantity_strategy(),
        side in side_strategy(),
    ) {
        let mut exchange = Exchange::new();
        let result = exchange.submit_limit(side, price, qty, TimeInForce::IOC);

        // IOC should never have resting quantity
        prop_assert_eq!(
            result.resting_quantity, 0,
            "IOC order has resting quantity: {}",
            result.resting_quantity
        );
    }

    /// FOK orders are either fully filled or not at all
    #[test]
    fn fok_all_or_nothing(
        resting_qty in quantity_strategy(),
        incoming_qty in quantity_strategy(),
        price in price_strategy(),
    ) {
        let mut exchange = Exchange::new();

        // Add some liquidity
        exchange.submit_limit(Side::Sell, price, resting_qty, TimeInForce::GTC);

        // Submit FOK buy
        let result = exchange.submit_limit(Side::Buy, price, incoming_qty, TimeInForce::FOK);

        // Either fully filled or not at all
        prop_assert!(
            result.filled_quantity == incoming_qty || result.filled_quantity == 0,
            "FOK partially filled: {} of {}",
            result.filled_quantity, incoming_qty
        );
    }

    // ========================================================================
    // DETERMINISM INVARIANTS
    // ========================================================================

    /// Same sequence of operations produces same results
    #[test]
    fn deterministic_replay(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy(), tif_strategy()),
            1..50
        )
    ) {
        // Run once
        let mut exchange1 = Exchange::new();
        let mut results1 = Vec::new();
        for (side, price, qty, tif) in &orders {
            let result = exchange1.submit_limit(*side, *price, *qty, *tif);
            results1.push((result.order_id, result.trades.len(), result.filled_quantity));
        }

        // Run again with same inputs
        let mut exchange2 = Exchange::new();
        let mut results2 = Vec::new();
        for (side, price, qty, tif) in &orders {
            let result = exchange2.submit_limit(*side, *price, *qty, *tif);
            results2.push((result.order_id, result.trades.len(), result.filled_quantity));
        }

        prop_assert_eq!(results1, results2, "non-deterministic behavior");
    }

    // ========================================================================
    // CANCEL INVARIANTS
    // ========================================================================

    /// Cancelling an order removes it from the book
    #[test]
    fn cancel_removes_order(
        price in price_strategy(),
        qty in quantity_strategy(),
        side in side_strategy(),
    ) {
        let mut exchange = Exchange::new();

        let result = exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        let order_id = result.order_id;

        // Only cancel if order is resting (not fully filled)
        if result.resting_quantity > 0 {
            let cancel_result = exchange.cancel(order_id);
            prop_assert!(cancel_result.success, "cancel failed");

            // Try to cancel again - should fail
            let second_cancel = exchange.cancel(order_id);
            prop_assert!(!second_cancel.success, "double cancel succeeded");
        }
    }

    // ========================================================================
    // SPREAD INVARIANTS
    // ========================================================================

    /// Spread is always non-negative
    #[test]
    fn non_negative_spread(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            1..100
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);

            // Check spread after each order
            if let Some(spread) = exchange.spread() {
                prop_assert!(
                    spread >= 0,
                    "negative spread: {}",
                    spread
                );
            }
        }
    }

    // ========================================================================
    // DEPTH SNAPSHOT INVARIANTS
    // ========================================================================

    /// Depth snapshot is sorted correctly
    #[test]
    fn depth_sorted_correctly(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            1..50
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        }

        let snapshot = exchange.depth(100);

        // Bids should be descending (best = highest first)
        for window in snapshot.bids.windows(2) {
            prop_assert!(
                window[0].price >= window[1].price,
                "bids not descending: {} < {}",
                window[0].price.0, window[1].price.0
            );
        }

        // Asks should be ascending (best = lowest first)
        for window in snapshot.asks.windows(2) {
            prop_assert!(
                window[0].price <= window[1].price,
                "asks not ascending: {} > {}",
                window[0].price.0, window[1].price.0
            );
        }
    }

    // ========================================================================
    // TRADE INVARIANTS
    // ========================================================================

    /// Trade IDs are always sequential
    #[test]
    fn trade_ids_sequential(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            2..20
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        }

        let trades = exchange.trades();
        for window in trades.windows(2) {
            prop_assert!(
                window[1].id.0 > window[0].id.0,
                "trade IDs not sequential: {} >= {}",
                window[0].id.0, window[1].id.0
            );
        }
    }

    /// Trade timestamps are monotonic
    #[test]
    fn trade_timestamps_monotonic(
        orders in prop::collection::vec(
            (side_strategy(), price_strategy(), quantity_strategy()),
            2..20
        )
    ) {
        let mut exchange = Exchange::new();

        for (side, price, qty) in orders {
            exchange.submit_limit(side, price, qty, TimeInForce::GTC);
        }

        let trades = exchange.trades();
        for window in trades.windows(2) {
            prop_assert!(
                window[1].timestamp > window[0].timestamp,
                "trade timestamps not monotonic: {} >= {}",
                window[0].timestamp, window[1].timestamp
            );
        }
    }
}

// ============================================================================
// REGRESSION TESTS (from proptest failures)
// ============================================================================

#[test]
fn regression_empty_book_depth() {
    let exchange = Exchange::new();
    let snapshot = exchange.depth(10);
    assert!(snapshot.bids.is_empty());
    assert!(snapshot.asks.is_empty());
}

#[test]
fn regression_single_order_depth() {
    let mut exchange = Exchange::new();
    exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

    let snapshot = exchange.depth(10);
    assert_eq!(snapshot.bids.len(), 1);
    assert_eq!(snapshot.bids[0].price, Price(100_00));
    assert_eq!(snapshot.bids[0].quantity, 100);
    assert!(snapshot.asks.is_empty());
}
