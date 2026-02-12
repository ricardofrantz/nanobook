//! Pre-trade risk checks.
//!
//! Validates a set of rebalance orders against risk limits before execution.

use nanobook::Symbol;
use nanobook_broker::{Account, BrokerSide};
use rustc_hash::{FxHashMap, FxHashSet};

use nanobook_risk::{RiskConfig as SharedRiskConfig, RiskEngine as RiskEngineImpl};

use crate::config::RiskConfig;
use crate::diff::{Action, RebalanceOrder};

pub use nanobook_risk::report::{RiskCheck, RiskReport, RiskStatus};

/// Convert rebalancer actions to broker-side direction.
fn action_to_side(action: Action) -> BrokerSide {
    match action {
        Action::Buy | Action::BuyCover => BrokerSide::Buy,
        Action::Sell | Action::SellShort => BrokerSide::Sell,
    }
}

/// Convert rebalancer risk config into nanobook-risk config.
fn adapt_config(config: &RiskConfig) -> SharedRiskConfig {
    let mut shared = SharedRiskConfig::default();
    shared.max_position_pct = config.max_position_pct;
    shared.max_leverage = config.max_leverage;
    shared.min_trade_usd = config.min_trade_usd;
    shared.max_trade_usd = config.max_trade_usd;
    shared.allow_short = config.allow_short;
    shared.max_short_pct = config.max_short_pct;
    // Rebalancer config doesn't expose these yet; 0 = disabled.
    shared.max_order_value_cents = 0;
    shared.max_batch_value_cents = 0;
    shared
}

fn validation_failure(detail: impl Into<String>) -> RiskReport {
    RiskReport {
        checks: vec![RiskCheck {
            name: "Order validation",
            status: RiskStatus::Fail,
            detail: detail.into(),
        }],
    }
}

/// Run all pre-trade risk checks.
///
/// # Arguments
/// - `orders`: The computed rebalance orders
/// - `equity_cents`: Total account equity
/// - `targets`: Target (symbol, weight) pairs
/// - `prices`: Current market prices (symbol, price_cents)
/// - `current_qty`: Current positions (symbol → quantity) including the effect of orders
/// - `config`: Risk configuration
pub fn check_risk(
    orders: &[RebalanceOrder],
    equity_cents: i64,
    targets: &[(Symbol, f64)],
    prices: &[(Symbol, i64)],
    current_qty: &FxHashMap<Symbol, i64>,
    config: &RiskConfig,
) -> RiskReport {
    let engine = RiskEngineImpl::new(adapt_config(config));
    let account = Account {
        equity_cents,
        buying_power_cents: equity_cents,
        cash_cents: equity_cents,
        gross_position_value_cents: 0,
    };

    let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
    let mut broker_orders: Vec<(Symbol, BrokerSide, u64, i64)> =
        Vec::with_capacity(orders.len() + current_qty.len());
    let mut symbols_with_orders: FxHashSet<Symbol> = FxHashSet::default();

    for order in orders {
        let shares = match u64::try_from(order.shares) {
            Ok(v) => v,
            Err(_) => {
                return validation_failure(format!("invalid share quantity for {order:?}"));
            }
        };

        broker_orders.push((
            order.symbol,
            action_to_side(order.action),
            shares,
            order.limit_price_cents,
        ));
        symbols_with_orders.insert(order.symbol);
    }

    // Include unchanged current positions by carrying a zero-quantity seed with a known price.
    // This keeps leverage/short exposure checks consistent with pre-trade behavior.
    for (symbol, qty) in current_qty.iter().filter(|(_, qty)| **qty != 0) {
        if symbols_with_orders.contains(symbol) {
            continue;
        }
        let price = price_map.get(symbol).copied().unwrap_or(0);
        let side = if *qty >= 0 {
            BrokerSide::Buy
        } else {
            BrokerSide::Sell
        };
        broker_orders.push((*symbol, side, 0, price));
    }

    let current_positions: Vec<(Symbol, i64)> =
        current_qty.iter().map(|(sym, qty)| (*sym, *qty)).collect();

    engine.check_batch(&broker_orders, &account, &current_positions, targets)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::{Action, RebalanceOrder};

    fn default_risk_config() -> RiskConfig {
        RiskConfig {
            max_position_pct: 0.40,
            max_leverage: 1.5,
            min_trade_usd: 100.0,
            max_trade_usd: 100_000.0,
            allow_short: true,
            max_short_pct: 0.30,
        }
    }

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }
    fn spy() -> Symbol {
        Symbol::new("SPY")
    }

    #[test]
    fn all_pass_simple() {
        let orders = vec![RebalanceOrder {
            symbol: aapl(),
            action: Action::Buy,
            shares: 100,
            limit_price_cents: 185_00,
            notional_cents: 1_850_000,
            description: "open",
        }];

        let targets = vec![(aapl(), 0.30)];
        let prices = vec![(aapl(), 185_00)];
        let current: FxHashMap<Symbol, i64> = FxHashMap::default();

        let report = check_risk(
            &orders,
            10_000_000, // $100K
            &targets,
            &prices,
            &current,
            &default_risk_config(),
        );

        assert!(!report.has_failures());
    }

    #[test]
    fn fail_max_position() {
        let orders = vec![RebalanceOrder {
            symbol: aapl(),
            action: Action::Buy,
            shares: 500,
            limit_price_cents: 185_00,
            notional_cents: 9_250_000,
            description: "open",
        }];

        let targets = vec![(aapl(), 0.50)]; // 50% > 40% limit
        let prices = vec![(aapl(), 185_00)];
        let current: FxHashMap<Symbol, i64> = FxHashMap::default();

        let report = check_risk(
            &orders,
            10_000_000,
            &targets,
            &prices,
            &current,
            &default_risk_config(),
        );

        assert!(report.has_failures());
    }

    #[test]
    fn fail_short_not_allowed() {
        let mut config = default_risk_config();
        config.allow_short = false;

        let orders = vec![RebalanceOrder {
            symbol: spy(),
            action: Action::SellShort,
            shares: 50,
            limit_price_cents: 430_00,
            notional_cents: 2_150_000,
            description: "open short",
        }];

        let targets = vec![(spy(), -0.10)];
        let prices = vec![(spy(), 430_00)];
        let current: FxHashMap<Symbol, i64> = FxHashMap::default();

        let report = check_risk(&orders, 10_000_000, &targets, &prices, &current, &config);

        assert!(report.has_failures());
    }

    #[test]
    fn warn_max_trade_size() {
        let orders = vec![RebalanceOrder {
            symbol: aapl(),
            action: Action::Buy,
            shares: 1000,
            limit_price_cents: 185_00,
            notional_cents: 18_500_000, // $185K > $100K max
            description: "open",
        }];

        let targets = vec![(aapl(), 0.30)];
        let prices = vec![(aapl(), 185_00)];
        let current: FxHashMap<Symbol, i64> = FxHashMap::default();

        let report = check_risk(
            &orders,
            100_000_000,
            &targets,
            &prices,
            &current,
            &default_risk_config(),
        );

        assert!(report.has_warnings());
        assert!(!report.has_failures());
    }

    #[test]
    fn display_report() {
        let report = RiskReport {
            checks: vec![RiskCheck {
                name: "Test",
                status: RiskStatus::Pass,
                detail: "ok".into(),
            }],
        };
        let s = format!("{report}");
        assert!(s.contains("[PASS]"));
    }
}
