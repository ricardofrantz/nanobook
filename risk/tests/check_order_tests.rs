// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Tests for RiskEngine::check_order — currently has ZERO tests.

use nanobook::Symbol;
use nanobook_broker::{Account, BrokerSide};
use nanobook_risk::{RiskConfig, RiskEngine, RiskStatus};

fn aapl() -> Symbol {
    Symbol::new("AAPL")
}

fn account(equity: i64) -> Account {
    Account {
        equity_cents: equity,
        buying_power_cents: equity,
        cash_cents: equity,
        gross_position_value_cents: 0,
    }
}

fn engine() -> RiskEngine {
    RiskEngine::new(RiskConfig {
        max_position_pct: 0.25,
        max_trade_usd: 50_000.0,
        ..RiskConfig::default()
    })
}

// ============================================================================
// Basic pass/fail
// ============================================================================

#[test]
fn small_order_passes() {
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        10,
        150_00,
        &account(10_000_000), // $100K equity
        &[],
    );
    assert!(!report.has_failures());
}

#[test]
fn large_position_fails() {
    // 500 shares * $150 = $75K = 75% of $100K equity > 25% limit
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        500,
        150_00,
        &account(10_000_000),
        &[],
    );
    assert!(report.has_failures());
}

#[test]
fn boundary_exactly_at_limit_passes() {
    // 25% of $100K = $25K. 166 shares * $150.50 ≈ $24,983 < 25%
    // We need exact: 25% of 10_000_000 = 2_500_000 cents
    // 2_500_000 / 150_00 = 166.67 → 166 shares → 166*15000=2_490_000 → 24.9% → Pass
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        166,
        150_00,
        &account(10_000_000),
        &[],
    );
    assert!(!report.has_failures());
}

#[test]
fn boundary_one_share_over_fails() {
    // 167 shares * $150 = $25,050 → 25.05% > 25% → Fail
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        167,
        150_00,
        &account(10_000_000),
        &[],
    );
    assert!(report.has_failures());
}

// ============================================================================
// Existing position affects check
// ============================================================================

#[test]
fn buy_increases_existing_long() {
    // Already have 100 shares long. Buying 100 more → 200 * $150 = $30K → 30% > 25% → Fail
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        100,
        150_00,
        &account(10_000_000),
        &[(aapl(), 100)],
    );
    assert!(report.has_failures());
}

#[test]
fn sell_reduces_existing_long() {
    // Have 200 shares long. Selling 100 → 100 * $150 = $15K → 15% < 25% → Pass
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Sell,
        100,
        150_00,
        &account(10_000_000),
        &[(aapl(), 200)],
    );
    assert!(!report.has_failures());
}

// ============================================================================
// Short selling checks
// ============================================================================

#[test]
fn short_allowed_passes() {
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Sell,
        10,
        150_00,
        &account(10_000_000),
        &[], // selling from flat = going short
    );
    // Default allows short
    assert!(!report.has_failures());
}

#[test]
fn short_not_allowed_fails() {
    let eng = RiskEngine::new(RiskConfig {
        max_position_pct: 0.25,
        allow_short: false,
        ..RiskConfig::default()
    });
    let report = eng.check_order(
        &aapl(),
        BrokerSide::Sell,
        10,
        150_00,
        &account(10_000_000),
        &[], // this creates a short
    );
    assert!(report.has_failures());
}

// ============================================================================
// Zero equity / zero quantity / zero price
// ============================================================================

#[test]
fn zero_equity_does_not_panic() {
    let report = engine().check_order(&aapl(), BrokerSide::Buy, 10, 150_00, &account(0), &[]);
    // Should not panic; position check defaults to 0%
    assert!(!report.has_failures());
}

#[test]
fn zero_quantity_does_not_panic() {
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        0,
        150_00,
        &account(10_000_000),
        &[],
    );
    assert!(!report.has_failures());
}

#[test]
fn zero_price_does_not_panic() {
    let report = engine().check_order(&aapl(), BrokerSide::Buy, 100, 0, &account(10_000_000), &[]);
    assert!(!report.has_failures());
}

// ============================================================================
// Order size warnings
// ============================================================================

#[test]
fn large_order_warns() {
    // 1000 shares * $150 = $150K > $50K max_trade_usd → Warn
    let report = engine().check_order(
        &aapl(),
        BrokerSide::Buy,
        1000,
        150_00,
        &account(100_000_000), // $1M equity (so position % is fine)
        &[],
    );
    assert!(report.has_warnings());
}

// ============================================================================
// RiskConfig::validate
// ============================================================================

#[test]
fn default_config_validates() {
    assert!(RiskConfig::default().validate().is_ok());
}

#[test]
fn nan_max_position_fails_validation() {
    let mut config = RiskConfig::default();
    config.max_position_pct = f64::NAN;
    assert!(config.validate().is_err());
}

#[test]
fn inf_max_leverage_fails_validation() {
    let mut config = RiskConfig::default();
    config.max_leverage = f64::INFINITY;
    assert!(config.validate().is_err());
}

#[test]
fn negative_max_drawdown_fails_validation() {
    let mut config = RiskConfig::default();
    config.max_drawdown_pct = -0.01;
    assert!(config.validate().is_err());
}

#[test]
fn zero_max_position_fails_validation() {
    let mut config = RiskConfig::default();
    config.max_position_pct = 0.0;
    assert!(config.validate().is_err());
}

#[test]
fn leverage_below_one_fails_validation() {
    let mut config = RiskConfig::default();
    config.max_leverage = 0.5;
    assert!(config.validate().is_err());
}

#[test]
fn boundary_values_validate() {
    let config = RiskConfig {
        max_position_pct: 1.0, // exactly 100%
        max_leverage: 1.0,     // exactly 1x
        max_drawdown_pct: 0.0, // zero drawdown allowed
        max_short_pct: 0.0,    // no short allowed
        min_trade_usd: 0.0,    // zero minimum
        max_trade_usd: 0.0,    // zero maximum
        ..RiskConfig::default()
    };
    assert!(config.validate().is_ok());
}

#[test]
#[should_panic(expected = "invalid RiskConfig")]
fn risk_engine_panics_on_invalid_config() {
    let mut config = RiskConfig::default();
    config.max_position_pct = f64::NAN;
    RiskEngine::new(config);
}
