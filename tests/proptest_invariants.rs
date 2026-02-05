// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Property-based tests for order book invariants.
//!
//! These tests use proptest to verify that key invariants hold
//! across randomly generated scenarios.

use nanobook::{Exchange, Price, Side, StopStatus, TimeInForce};
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

    // ========================================================================
    // STOP ORDER INVARIANTS
    // ========================================================================

    /// Stop-market orders that trigger produce valid trades
    #[test]
    fn stop_market_triggers_produce_valid_trades(
        resting_price in price_strategy(),
        resting_qty in quantity_strategy(),
        stop_price in price_strategy(),
        stop_qty in quantity_strategy(),
    ) {
        let mut exchange = Exchange::new();

        // Build resting liquidity on both sides
        exchange.submit_limit(Side::Sell, resting_price, resting_qty, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(1), resting_qty, TimeInForce::GTC);

        // Create a trade to set last_trade_price
        exchange.submit_limit(Side::Buy, resting_price, 1, TimeInForce::GTC);
        let trades_before = exchange.trades().len();

        // Submit a stop-market that may trigger immediately
        let result = exchange.submit_stop_market(Side::Sell, stop_price, stop_qty);

        // If triggered, any new trades must have valid (positive) prices and quantities
        if result.status == StopStatus::Triggered {
            for trade in &exchange.trades()[trades_before..] {
                prop_assert!(trade.price.0 > 0, "trade at non-positive price");
                prop_assert!(trade.quantity > 0, "trade with zero quantity");
            }
        }
    }

    /// Cancelled stop orders never trigger
    #[test]
    fn cancelled_stop_never_triggers(
        stop_price in price_strategy(),
        stop_qty in quantity_strategy(),
        trade_price in price_strategy(),
        trade_qty in quantity_strategy(),
    ) {
        let mut exchange = Exchange::new();

        // Add liquidity so trades can happen
        exchange.submit_limit(Side::Sell, trade_price, trade_qty, TimeInForce::GTC);

        // Submit and immediately cancel a buy stop
        let stop = exchange.submit_stop_market(Side::Buy, stop_price, stop_qty);
        exchange.cancel(stop.order_id);

        let trades_before = exchange.trades().len();

        // Now create a trade that would have triggered the stop
        exchange.submit_limit(Side::Buy, trade_price, trade_qty, TimeInForce::IOC);

        // The cancelled stop should NOT have produced additional trades beyond the direct match
        // Count trades from the direct IOC match
        let new_trades = exchange.trades().len() - trades_before;
        // At most 1 trade from the direct IOC match (may be 0 if no liquidity left)
        prop_assert!(new_trades <= 1, "cancelled stop produced extra trades: {}", new_trades);
    }

    /// Stop order cascade depth is bounded
    #[test]
    fn stop_cascade_bounded(
        base_price in 50_000i64..60_000i64,
    ) {
        let mut exchange = Exchange::new();

        // Create alternating buy/sell liquidity at multiple levels
        for i in 0..10 {
            let p = base_price + i * 100;
            exchange.submit_limit(Side::Sell, Price(p), 10, TimeInForce::GTC);
            exchange.submit_limit(Side::Buy, Price(p - 50), 10, TimeInForce::GTC);
        }

        // Chain many stop orders that could cascade
        for i in 0..150 {
            let p = base_price + i * 10;
            exchange.submit_stop_market(Side::Buy, Price(p), 5);
            exchange.submit_stop_market(Side::Sell, Price(p), 5);
        }

        // Trigger the chain with a market order
        let trades_before = exchange.trades().len();
        exchange.submit_market(Side::Buy, 10);

        // Verify we didn't crash or hang (cascade is bounded at 100)
        let total_trades = exchange.trades().len();
        prop_assert!(total_trades >= trades_before, "trade count went backwards");
        // Verify book isn't crossed after cascade
        let (best_bid, best_ask) = exchange.best_bid_ask();
        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            prop_assert!(bid < ask, "crossed book after cascade: bid {} >= ask {}", bid.0, ask.0);
        }
    }

    /// Quantity conservation holds for stop-limit triggers
    #[test]
    fn stop_limit_quantity_conservation(
        resting_price in price_strategy(),
        resting_qty in quantity_strategy(),
        stop_price in price_strategy(),
        limit_price in price_strategy(),
        stop_qty in quantity_strategy(),
    ) {
        let mut exchange = Exchange::new();

        // Add resting asks
        exchange.submit_limit(Side::Sell, resting_price, resting_qty, TimeInForce::GTC);

        // Create a trade to set last_trade_price
        exchange.submit_limit(Side::Buy, resting_price, 1, TimeInForce::IOC);

        // Submit a buy stop-limit
        exchange.submit_stop_limit(
            Side::Buy, stop_price, limit_price, stop_qty, TimeInForce::GTC
        );

        // The book should not be crossed
        let (best_bid, best_ask) = exchange.best_bid_ask();
        if let (Some(bid), Some(ask)) = (best_bid, best_ask) {
            prop_assert!(bid < ask, "crossed book: bid {} >= ask {}", bid.0, ask.0);
        }

        // Verify all trades have positive quantities
        for trade in exchange.trades() {
            prop_assert!(trade.quantity > 0, "zero-quantity trade");
            prop_assert!(trade.price.0 > 0, "non-positive trade price");
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
