//! Individual risk check implementations.

use nanobook::Symbol;
use nanobook_broker::{Account, BrokerSide};
use rustc_hash::FxHashMap;

use crate::config::RiskConfig;
use crate::report::{RiskCheck, RiskReport, RiskStatus};

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
            if pos_status == RiskStatus::Pass {
                "<="
            } else {
                ">"
            },
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

    let gross_exposure: i64 = post_qty
        .iter()
        .map(|(sym, qty)| {
            let price = price_map.get(sym).copied().unwrap_or(0);
            qty.abs() * price
        })
        .sum();
    let leverage = if equity > 0 {
        gross_exposure as f64 / equity as f64
    } else {
        0.0
    };
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
            if lev_status == RiskStatus::Pass {
                "<="
            } else {
                ">"
            },
            config.max_leverage,
        ),
    });

    // 3. Short exposure check
    let short_exposure: i64 = post_qty
        .iter()
        .filter(|(_, qty)| **qty < 0)
        .map(|(sym, qty)| {
            let price = price_map.get(sym).copied().unwrap_or(0);
            qty.abs() * price
        })
        .sum();
    let short_pct = if equity > 0 {
        short_exposure as f64 / equity as f64
    } else {
        0.0
    };

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
                if short_status == RiskStatus::Pass {
                    "<="
                } else {
                    ">"
                },
                config.max_short_pct * 100.0,
            ),
        });
    }

    // 4. Max trade size — warn if any trade > max_trade_usd
    let max_cents = (config.max_trade_usd * 100.0) as i64;
    for &(sym, _side, qty, price) in orders {
        let qty_i64 = i64::try_from(qty).unwrap_or(i64::MAX);
        let notional = qty_i64.saturating_mul(price);
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

    // 5. Order count
    checks.push(RiskCheck {
        name: "Order count",
        status: RiskStatus::Pass,
        detail: format!("{} orders", orders.len()),
    });

    // 6. Target weights sanity
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
        let report = check_batch(
            &default_config(),
            &orders,
            &account(100_000_000),
            &[],
            &targets,
        );
        assert!(report.has_warnings());
        assert!(!report.has_failures());
    }
}
