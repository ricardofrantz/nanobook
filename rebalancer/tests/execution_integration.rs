// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Integration tests for rebalancer execution helpers.

use nanobook::Symbol;
use nanobook_broker::BrokerSide;
use nanobook_rebalancer::diff::{Action, CurrentPosition};
use nanobook_rebalancer::error::Error;
use nanobook_rebalancer::execution::{
    action_to_side, apply_constraint_overrides, collect_all_symbols, enforce_max_orders_per_run,
};
use nanobook_rebalancer::target::TargetSpec;

fn aapl() -> Symbol {
    Symbol::new("AAPL")
}
fn msft() -> Symbol {
    Symbol::new("MSFT")
}
fn spy() -> Symbol {
    Symbol::new("SPY")
}

fn valid_target_json() -> &'static str {
    r#"{
        "timestamp": "2026-02-08T15:30:00Z",
        "targets": [
            { "symbol": "AAPL", "weight": 0.40 },
            { "symbol": "MSFT", "weight": 0.30 }
        ]
    }"#
}

// ============================================================================
// action_to_side
// ============================================================================

#[test]
fn action_buy_maps_to_buy() {
    assert!(matches!(action_to_side(Action::Buy), BrokerSide::Buy));
}

#[test]
fn action_sell_maps_to_sell() {
    assert!(matches!(action_to_side(Action::Sell), BrokerSide::Sell));
}

#[test]
fn action_buy_cover_maps_to_buy() {
    assert!(matches!(action_to_side(Action::BuyCover), BrokerSide::Buy));
}

#[test]
fn action_sell_short_maps_to_sell() {
    assert!(matches!(
        action_to_side(Action::SellShort),
        BrokerSide::Sell
    ));
}

// ============================================================================
// collect_all_symbols
// ============================================================================

#[test]
fn collect_union_of_positions_and_targets() {
    let positions = vec![
        CurrentPosition {
            symbol: aapl(),
            quantity: 100,
            avg_cost_cents: 150_00,
        },
        CurrentPosition {
            symbol: spy(),
            quantity: 50,
            avg_cost_cents: 430_00,
        },
    ];
    let target = TargetSpec::from_json(valid_target_json()).unwrap();

    let symbols = collect_all_symbols(&positions, &target);

    // Should contain AAPL (from both), SPY (from positions), MSFT (from targets)
    assert!(symbols.contains(&aapl()));
    assert!(symbols.contains(&msft()));
    assert!(symbols.contains(&spy()));
    // No duplicates
    assert_eq!(symbols.iter().filter(|s| **s == aapl()).count(), 1);
}

#[test]
fn collect_empty_positions() {
    let target = TargetSpec::from_json(valid_target_json()).unwrap();
    let symbols = collect_all_symbols(&[], &target);
    assert_eq!(symbols.len(), 2); // AAPL, MSFT from target
}

// ============================================================================
// apply_constraint_overrides
// ============================================================================

#[test]
fn override_max_position() {
    let base = nanobook_rebalancer::config::RiskConfig {
        max_position_pct: 0.25,
        max_leverage: 1.5,
        min_trade_usd: 100.0,
        ..Default::default()
    };

    let json = r#"{
        "timestamp": "2026-02-08T15:30:00Z",
        "targets": [{ "symbol": "AAPL", "weight": 0.5 }],
        "constraints": { "max_position_pct": 0.50 }
    }"#;
    let target = TargetSpec::from_json(json).unwrap();

    let overridden = apply_constraint_overrides(&base, &target);
    assert_eq!(overridden.max_position_pct, 0.50);
    // Other fields unchanged
    assert_eq!(overridden.max_leverage, 1.5);
    assert_eq!(overridden.min_trade_usd, 100.0);
}

#[test]
fn no_constraints_returns_base() {
    let base = nanobook_rebalancer::config::RiskConfig {
        max_position_pct: 0.25,
        ..Default::default()
    };

    let target = TargetSpec::from_json(valid_target_json()).unwrap();
    let result = apply_constraint_overrides(&base, &target);
    assert_eq!(result.max_position_pct, 0.25);
}

// ============================================================================
// diff::compute_diff (integration through the public API)
// ============================================================================

#[test]
fn compute_diff_no_changes_needed() {
    use nanobook_rebalancer::diff::compute_diff;

    // Portfolio perfectly matches target
    let current = vec![CurrentPosition {
        symbol: aapl(),
        quantity: 2702,
        avg_cost_cents: 185_00,
    }];

    let orders = compute_diff(
        1_000_000_00,
        &current,
        &[(aapl(), 0.5)],
        &[(aapl(), 185_00)],
        0,
        100_00, // $100 min trade
    );

    assert!(orders.is_empty());
}

#[test]
fn compute_diff_empty_everything() {
    use nanobook_rebalancer::diff::compute_diff;

    let orders = compute_diff(1_000_000_00, &[], &[], &[], 0, 0);

    assert!(orders.is_empty());
}

#[test]
fn compute_diff_zero_equity() {
    use nanobook_rebalancer::diff::compute_diff;

    let orders = compute_diff(
        0, // zero equity
        &[],
        &[(aapl(), 0.5)],
        &[(aapl(), 150_00)],
        0,
        0,
    );

    // 0 equity → target_value = 0 → diff = 0 → no orders
    assert!(orders.is_empty());
}

#[test]
fn enforce_max_orders_per_run_allows_under_limit() {
    let result = enforce_max_orders_per_run(3, 5);
    assert!(result.is_ok());
}

#[test]
fn enforce_max_orders_per_run_rejects_over_limit() {
    let result = enforce_max_orders_per_run(10, 5);
    match result {
        Err(Error::RiskFailed(msg)) => {
            assert!(msg.contains("10 orders generated"));
            assert!(msg.contains("max_orders_per_run is 5"));
        }
        _ => panic!("expected RiskFailed"),
    }
}
