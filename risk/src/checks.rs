//! Individual risk check implementations.

use nanobook::Symbol;
use nanobook_broker::{Account, BrokerSide};
use rustc_hash::FxHashMap;

use crate::config::RiskConfig;
use crate::report::{RiskCheck, RiskReport, RiskStatus};

/// Returns `"<="` if the check passed, `">"` if it failed or warned.
fn cmp_symbol(status: RiskStatus) -> &'static str {
    if status == RiskStatus::Pass {
        "<="
    } else {
        ">"
    }
}

/// Ratio of `numerator / equity`, or `INFINITY` when equity is non-positive.
fn ratio_or_inf(numerator: i64, equity: i64) -> f64 {
    if equity > 0 {
        numerator as f64 / equity as f64
    } else {
        f64::INFINITY
    }
}

/// Run all risk checks for a batch of orders.
pub fn check_batch(
    config: &RiskConfig,
    orders: &[(Symbol, BrokerSide, u64, i64)],
    account: &Account,
    current_positions: &[(Symbol, i64)],
    target_weights: &[(Symbol, f64)],
) -> RiskReport {
    let equity = account.equity_cents;
    let target_map: FxHashMap<Symbol, f64> = target_weights.iter().copied().collect();
    let mut checks = Vec::new();

    // 1. Max position check — no single target weight > max_position_pct
    let max_pos = config.max_position_pct;
    let mut worst_pos = 0.0_f64;
    let mut worst_sym = Symbol::new("?");
    for &(sym, weight) in target_weights {
        let abs_w = weight.abs();
        if abs_w > worst_pos {
            worst_pos = abs_w;
            worst_sym = sym;
        }
    }
    let pos_status = if worst_pos > max_pos {
        RiskStatus::Fail
    } else {
        RiskStatus::Pass
    };
    checks.push(RiskCheck {
        name: "Max position",
        status: pos_status,
        detail: format!(
            "{:.1}% ({}) {} {:.1}% limit",
            worst_pos * 100.0,
            worst_sym.as_str(),
            cmp_symbol(pos_status),
            max_pos * 100.0,
        ),
    });

    // 2. Leverage check — post-trade gross exposure / equity
    let current_map: FxHashMap<Symbol, i64> = current_positions.iter().copied().collect();
    let mut post_qty = current_map.clone();
    let mut price_map: FxHashMap<Symbol, i64> = FxHashMap::default();
    for &(sym, side, qty, price) in orders {
        let sign = match side {
            BrokerSide::Buy => 1_i64,
            BrokerSide::Sell => -1,
        };
        let qty_i64 = i64::try_from(qty).unwrap_or(i64::MAX);
        *post_qty.entry(sym).or_insert(0) += sign.saturating_mul(qty_i64);
        price_map.insert(sym, price);
    }

    // Compute exposure: sum of |qty| * |price| for each position.
    // When `filter` is None, includes all positions (gross); when Some(f),
    // includes only positions where qty satisfies f.
    let exposure = |filter: Option<fn(&i64) -> bool>| -> i64 {
        post_qty
            .iter()
            .filter(|(_, qty)| filter.is_none_or(|f| f(qty)))
            .map(|(sym, qty)| {
                let price = price_map.get(sym).copied().unwrap_or(0).saturating_abs();
                qty.saturating_abs().saturating_mul(price)
            })
            .fold(0_i64, |acc, v| acc.saturating_add(v))
    };

    let gross_exposure = exposure(None);
    let leverage = ratio_or_inf(gross_exposure, equity);
    let lev_status = if leverage > config.max_leverage {
        RiskStatus::Fail
    } else {
        RiskStatus::Pass
    };
    checks.push(RiskCheck {
        name: "Leverage",
        status: lev_status,
        detail: format!(
            "{:.2}x {} {:.2}x limit",
            leverage,
            cmp_symbol(lev_status),
            config.max_leverage,
        ),
    });

    // 3. Short exposure check
    let short_exposure = exposure(Some(|qty: &i64| *qty < 0));
    let short_pct = ratio_or_inf(short_exposure, equity);

    let has_shorts = orders
        .iter()
        .any(|(_, side, _, _)| *side == BrokerSide::Sell)
        && post_qty.values().any(|q| *q < 0);

    if has_shorts && !config.allow_short {
        checks.push(RiskCheck {
            name: "Short selling",
            status: RiskStatus::Fail,
            detail: "short selling not allowed".into(),
        });
    } else {
        let short_status = if short_pct > config.max_short_pct {
            RiskStatus::Fail
        } else {
            RiskStatus::Pass
        };
        checks.push(RiskCheck {
            name: "Short exposure",
            status: short_status,
            detail: format!(
                "{:.1}% {} {:.1}% limit",
                short_pct * 100.0,
                cmp_symbol(short_status),
                config.max_short_pct * 100.0,
            ),
        });
    }

    // 4. Max order value in cents
    let mut batch_value = 0_i64;
    for &(sym, _side, qty, price) in orders {
        let qty_i64 = i64::try_from(qty).unwrap_or(i64::MAX);
        let notional = qty_i64.saturating_mul(price.saturating_abs());
        batch_value = batch_value.saturating_add(notional);

        let max_order = config.max_order_value_cents;
        if max_order > 0 && notional > max_order {
            checks.push(RiskCheck {
                name: "Max order value",
                status: RiskStatus::Fail,
                detail: format!(
                    "{}: ${:.0} > ${:.0} max_order_value_cents",
                    sym,
                    notional as f64 / 100.0,
                    max_order as f64 / 100.0,
                ),
            });
        }
    }

    // 5. Max rebalance batch value in cents
    let max_batch = config.max_batch_value_cents;
    if max_batch > 0 && batch_value > max_batch {
        checks.push(RiskCheck {
            name: "Max batch value",
            status: RiskStatus::Fail,
            detail: format!(
                "${:.0} > ${:.0} max_batch_value_cents",
                batch_value as f64 / 100.0,
                max_batch as f64 / 100.0,
            ),
        });
    }

    // 6. Max trade size — warn if any trade > max_trade_usd
    let max_cents = (config.max_trade_usd * 100.0) as i64;
    for &(sym, _side, qty, price) in orders {
        let qty_i64 = i64::try_from(qty).unwrap_or(i64::MAX);
        let notional = qty_i64.saturating_mul(price.saturating_abs());
        if notional > max_cents {
            checks.push(RiskCheck {
                name: "Max trade size",
                status: RiskStatus::Warn,
                detail: format!(
                    "{}: ${:.0} > ${:.0} max_trade_usd",
                    sym,
                    notional as f64 / 100.0,
                    config.max_trade_usd,
                ),
            });
        }
    }

    // 7. Order count
    checks.push(RiskCheck {
        name: "Order count",
        status: RiskStatus::Pass,
        detail: format!("{} orders", orders.len()),
    });

    // 8. Target weights sanity
    let long_sum: f64 = target_map.values().filter(|w| **w > 0.0).sum();
    let short_sum: f64 = target_map
        .values()
        .filter(|w| **w < 0.0)
        .map(|w| w.abs())
        .sum();
    checks.push(RiskCheck {
        name: "Weight allocation",
        status: RiskStatus::Pass,
        detail: format!(
            "{:.1}% long, {:.1}% short, {:.1}% cash",
            long_sum * 100.0,
            short_sum * 100.0,
            (1.0 - long_sum) * 100.0,
        ),
    });

    RiskReport { checks }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }
    fn spy() -> Symbol {
        Symbol::new("SPY")
    }

    fn default_config() -> RiskConfig {
        RiskConfig {
            max_position_pct: 0.40,
            max_leverage: 1.5,
            allow_short: true,
            max_short_pct: 0.30,
            max_trade_usd: 100_000.0,
            ..RiskConfig::default()
        }
    }

    fn account(equity: i64) -> Account {
        Account {
            equity_cents: equity,
            buying_power_cents: equity,
            cash_cents: equity,
            gross_position_value_cents: 0,
        }
    }

    #[test]
    fn all_pass_simple() {
        let orders = vec![(aapl(), BrokerSide::Buy, 100_u64, 185_00_i64)];
        let targets = vec![(aapl(), 0.30)];
        let report = check_batch(
            &default_config(),
            &orders,
            &account(10_000_000),
            &[],
            &targets,
        );
        assert!(!report.has_failures());
    }

    #[test]
    fn fail_max_position() {
        let orders = vec![(aapl(), BrokerSide::Buy, 500, 185_00)];
        let targets = vec![(aapl(), 0.50)]; // 50% > 40%
        let report = check_batch(
            &default_config(),
            &orders,
            &account(10_000_000),
            &[],
            &targets,
        );
        assert!(report.has_failures());
    }

    #[test]
    fn fail_short_not_allowed() {
        let mut config = default_config();
        config.allow_short = false;

        let orders = vec![(spy(), BrokerSide::Sell, 50, 430_00)];
        let targets = vec![(spy(), -0.10)];
        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);
        assert!(report.has_failures());
    }

    #[test]
    fn warn_large_trade() {
        let orders = vec![(aapl(), BrokerSide::Buy, 1000, 185_00)];
        let targets = vec![(aapl(), 0.30)];
        let mut config = default_config();
        config.max_order_value_cents = 20_000_000; // keep order-level hard cap above max trade warning
        let report = check_batch(&config, &orders, &account(100_000_000), &[], &targets);
        assert!(report.has_warnings());
        assert!(!report.has_failures());
    }

    #[test]
    fn fails_max_order_value() {
        let orders = vec![(aapl(), BrokerSide::Buy, 120, 150_00)];
        let targets = vec![(aapl(), 0.30)];
        let mut config = default_config();
        config.max_order_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(report.has_failures());
    }

    #[test]
    fn passes_max_order_value_boundary() {
        let orders = vec![(aapl(), BrokerSide::Buy, 2, 5_000)];
        let targets = vec![(aapl(), 0.30)];
        let mut config = default_config();
        config.max_order_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(!report.has_failures());
        assert!(!report.checks.iter().any(|c| c.name == "Max order value"));
    }

    #[test]
    fn boundary_checks_include_failure_messages() {
        let orders = vec![(aapl(), BrokerSide::Buy, 3, 5_000)];
        let targets = vec![(aapl(), 0.30)];
        let mut config = default_config();
        config.max_order_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(report.has_failures());
        let order_limit = report
            .checks
            .iter()
            .find(|c| c.name == "Max order value")
            .unwrap();
        assert_eq!(order_limit.status, RiskStatus::Fail);
        assert!(
            order_limit
                .detail
                .contains("$150 > $100 max_order_value_cents")
        );
    }

    #[test]
    fn fails_max_batch_value() {
        let orders = vec![
            (aapl(), BrokerSide::Buy, 40, 185_00),
            (spy(), BrokerSide::Sell, 40, 185_00),
        ];
        let targets = vec![(aapl(), 0.30), (spy(), -0.30)];
        let mut config = default_config();
        config.max_batch_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(report.has_failures());
    }

    #[test]
    fn passes_max_batch_value_boundary() {
        let orders = vec![
            (aapl(), BrokerSide::Buy, 4, 2500),
            (spy(), BrokerSide::Sell, 4, 0),
        ];
        let targets = vec![(aapl(), 0.30), (spy(), -0.30)];
        let mut config = default_config();
        config.max_batch_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(!report.has_failures());
        assert!(!report.checks.iter().any(|c| c.name == "Max batch value"));
    }

    #[test]
    fn fails_batch_value_with_report_name() {
        let orders = vec![
            (aapl(), BrokerSide::Buy, 25, 500),
            (spy(), BrokerSide::Sell, 25, 500),
        ];
        let targets = vec![(aapl(), 0.30), (spy(), -0.30)];
        let mut config = default_config();
        config.max_batch_value_cents = 10_000; // $100

        let report = check_batch(&config, &orders, &account(10_000_000), &[], &targets);

        assert!(report.has_failures());
        let batch_limit = report
            .checks
            .iter()
            .find(|c| c.name == "Max batch value")
            .unwrap();
        assert_eq!(batch_limit.status, RiskStatus::Fail);
        assert!(
            batch_limit
                .detail
                .contains("$250 > $100 max_batch_value_cents")
        );
    }
}
