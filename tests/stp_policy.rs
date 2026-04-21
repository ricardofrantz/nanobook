// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00).
#![allow(clippy::inconsistent_digit_grouping)]

//! Integration tests for self-trade prevention (N8).
//!
//! Matrix:
//! - Policy ∈ {Off, CancelNewest, CancelOldest, DecrementAndCancel}
//! - Owner combinations: same, different, one-None, both-None
//!
//! The core invariant: STP only fires when BOTH orders set non-None
//! owners AND the owners compare equal. Otherwise the trade must execute
//! exactly as it would without STP configured.

use nanobook::{
    Exchange, OrderOwner, OrderStatus, Price, Side, StpPolicy, SubmitResult, TimeInForce,
};

const PX: Price = Price(100_00); // $100.00
const ALICE: OrderOwner = OrderOwner(1);
const BOB: OrderOwner = OrderOwner(2);

/// Seed `exchange` with a resting sell at PX owned by `resting_owner`.
fn rest_sell(exchange: &mut Exchange, qty: u64, resting_owner: Option<OrderOwner>) {
    match resting_owner {
        Some(o) => {
            exchange.submit_limit_with_owner(Side::Sell, PX, qty, TimeInForce::GTC, o);
        }
        None => {
            exchange.submit_limit(Side::Sell, PX, qty, TimeInForce::GTC);
        }
    }
}

/// Submit an incoming buy at PX against a fresh exchange, after seeding a
/// resting sell.
fn run(
    policy: StpPolicy,
    resting_owner: Option<OrderOwner>,
    incoming_owner: Option<OrderOwner>,
    resting_qty: u64,
    incoming_qty: u64,
) -> (Exchange, SubmitResult) {
    let mut exchange = Exchange::new().with_stp_policy(policy);
    rest_sell(&mut exchange, resting_qty, resting_owner);
    let result = match incoming_owner {
        Some(o) => {
            exchange.submit_limit_with_owner(Side::Buy, PX, incoming_qty, TimeInForce::GTC, o)
        }
        None => exchange.submit_limit(Side::Buy, PX, incoming_qty, TimeInForce::GTC),
    };
    (exchange, result)
}

// ============================================================================
// Opt-out: owner=None on either side never triggers STP.
// ============================================================================

#[test]
fn none_owner_on_incoming_always_trades() {
    for policy in [
        StpPolicy::Off,
        StpPolicy::CancelNewest,
        StpPolicy::CancelOldest,
        StpPolicy::DecrementAndCancel,
    ] {
        let (_ex, result) = run(policy, Some(ALICE), None, 100, 100);
        assert_eq!(result.filled_quantity, 100, "policy {:?}", policy);
        assert_eq!(result.trades.len(), 1, "policy {:?}", policy);
        assert_eq!(result.status, OrderStatus::Filled, "policy {:?}", policy);
    }
}

#[test]
fn none_owner_on_resting_always_trades() {
    for policy in [
        StpPolicy::Off,
        StpPolicy::CancelNewest,
        StpPolicy::CancelOldest,
        StpPolicy::DecrementAndCancel,
    ] {
        let (_ex, result) = run(policy, None, Some(ALICE), 100, 100);
        assert_eq!(result.filled_quantity, 100, "policy {:?}", policy);
        assert_eq!(result.status, OrderStatus::Filled, "policy {:?}", policy);
    }
}

#[test]
fn both_none_always_trades() {
    for policy in [
        StpPolicy::Off,
        StpPolicy::CancelNewest,
        StpPolicy::CancelOldest,
        StpPolicy::DecrementAndCancel,
    ] {
        let (_ex, result) = run(policy, None, None, 100, 100);
        assert_eq!(result.filled_quantity, 100, "policy {:?}", policy);
        assert_eq!(result.status, OrderStatus::Filled, "policy {:?}", policy);
    }
}

// ============================================================================
// Different owners: STP never fires.
// ============================================================================

#[test]
fn different_owners_always_trade() {
    for policy in [
        StpPolicy::Off,
        StpPolicy::CancelNewest,
        StpPolicy::CancelOldest,
        StpPolicy::DecrementAndCancel,
    ] {
        let (_ex, result) = run(policy, Some(ALICE), Some(BOB), 100, 100);
        assert_eq!(result.filled_quantity, 100, "policy {:?}", policy);
        assert_eq!(result.trades.len(), 1, "policy {:?}", policy);
        assert_eq!(result.status, OrderStatus::Filled, "policy {:?}", policy);
    }
}

// ============================================================================
// Same owner: policy governs.
// ============================================================================

#[test]
fn same_owner_off_trades_normally() {
    let (exchange, result) = run(StpPolicy::Off, Some(ALICE), Some(ALICE), 100, 100);
    assert_eq!(result.filled_quantity, 100);
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.status, OrderStatus::Filled);
    // Book is empty.
    assert_eq!(exchange.book().asks().total_quantity(), 0);
}

#[test]
fn same_owner_cancel_newest_cancels_incoming() {
    let (exchange, result) = run(StpPolicy::CancelNewest, Some(ALICE), Some(ALICE), 100, 100);
    // Incoming rejected wholesale; resting untouched.
    assert_eq!(result.filled_quantity, 0);
    assert_eq!(result.cancelled_quantity, 100);
    assert_eq!(result.resting_quantity, 0);
    assert_eq!(result.status, OrderStatus::Cancelled);
    assert!(result.trades.is_empty());
    // Resting sell still fully present.
    assert_eq!(exchange.book().asks().total_quantity(), 100);
}

#[test]
fn same_owner_cancel_oldest_cancels_resting_then_nothing_left() {
    let (exchange, result) = run(StpPolicy::CancelOldest, Some(ALICE), Some(ALICE), 100, 100);
    // Resting cancelled; incoming found no more liquidity, rests at PX.
    assert_eq!(result.filled_quantity, 0);
    assert!(result.trades.is_empty());
    // Incoming is GTC so it rests.
    assert_eq!(result.status, OrderStatus::New);
    assert_eq!(result.resting_quantity, 100);
    // Ask side cleared (resting cancelled); bid side has the incoming order.
    assert_eq!(exchange.book().asks().total_quantity(), 0);
    assert_eq!(exchange.book().bids().total_quantity(), 100);
}

#[test]
fn same_owner_cancel_oldest_continues_matching_against_other_owner() {
    // Build a book: ALICE's resting sell in front, BOB's behind at the same price.
    let mut exchange = Exchange::new().with_stp_policy(StpPolicy::CancelOldest);
    exchange.submit_limit_with_owner(Side::Sell, PX, 100, TimeInForce::GTC, ALICE);
    exchange.submit_limit_with_owner(Side::Sell, PX, 100, TimeInForce::GTC, BOB);

    // ALICE buys 100 → STP cancels ALICE's resting, then matches BOB's.
    let result = exchange.submit_limit_with_owner(Side::Buy, PX, 100, TimeInForce::GTC, ALICE);
    assert_eq!(result.filled_quantity, 100);
    assert_eq!(result.trades.len(), 1);
    // Trade counterparty is BOB's order (second OrderId).
    assert_eq!(result.trades[0].passive_order_id.0, 2);
    assert_eq!(result.status, OrderStatus::Filled);
    // Ask side empty (ALICE cancelled, BOB matched).
    assert_eq!(exchange.book().asks().total_quantity(), 0);
}

#[test]
fn same_owner_decrement_and_cancel_smaller_incoming() {
    // Incoming (50) < resting (100): incoming cancelled, resting preserved.
    let (exchange, result) = run(
        StpPolicy::DecrementAndCancel,
        Some(ALICE),
        Some(ALICE),
        100,
        50,
    );
    assert_eq!(result.filled_quantity, 0);
    assert_eq!(result.cancelled_quantity, 50);
    assert_eq!(result.status, OrderStatus::Cancelled);
    assert!(result.trades.is_empty());
    // Resting untouched.
    assert_eq!(exchange.book().asks().total_quantity(), 100);
}

#[test]
fn same_owner_decrement_and_cancel_smaller_resting() {
    // Incoming (100) > resting (50): resting cancelled, incoming keeps 100
    // and continues matching; with no further liquidity it rests as GTC.
    let (exchange, result) = run(
        StpPolicy::DecrementAndCancel,
        Some(ALICE),
        Some(ALICE),
        50,
        100,
    );
    assert_eq!(result.filled_quantity, 0);
    assert!(result.trades.is_empty());
    // Incoming rests in full (GTC + no liquidity left).
    assert_eq!(result.resting_quantity, 100);
    assert_eq!(result.status, OrderStatus::New);
    assert_eq!(exchange.book().asks().total_quantity(), 0);
    assert_eq!(exchange.book().bids().total_quantity(), 100);
}

#[test]
fn same_owner_decrement_and_cancel_equal_cancels_resting() {
    // Equal sizes: policy specifies resting cancelled, incoming continues.
    let (exchange, result) = run(
        StpPolicy::DecrementAndCancel,
        Some(ALICE),
        Some(ALICE),
        100,
        100,
    );
    assert_eq!(result.filled_quantity, 0);
    assert!(result.trades.is_empty());
    assert_eq!(result.resting_quantity, 100);
    assert_eq!(result.status, OrderStatus::New);
    assert_eq!(exchange.book().asks().total_quantity(), 0);
    assert_eq!(exchange.book().bids().total_quantity(), 100);
}

// ============================================================================
// Partial fill before STP: CancelNewest after trading against a non-owner
// ============================================================================

#[test]
fn cancel_newest_after_partial_fill_against_other_owner() {
    // Book: BOB's 30 first (best time priority), then ALICE's 100 behind.
    let mut exchange = Exchange::new().with_stp_policy(StpPolicy::CancelNewest);
    exchange.submit_limit_with_owner(Side::Sell, PX, 30, TimeInForce::GTC, BOB);
    exchange.submit_limit_with_owner(Side::Sell, PX, 100, TimeInForce::GTC, ALICE);

    // ALICE buys 100: trades 30 against BOB, then STP-cancels at ALICE's resting.
    let result = exchange.submit_limit_with_owner(Side::Buy, PX, 100, TimeInForce::GTC, ALICE);
    assert_eq!(result.filled_quantity, 30);
    assert_eq!(result.cancelled_quantity, 70);
    assert_eq!(result.status, OrderStatus::PartiallyFilled);
    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.trades[0].quantity, 30);
    // ALICE's resting sell untouched (still 100 on ask side).
    assert_eq!(exchange.book().asks().total_quantity(), 100);
}
