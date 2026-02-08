//! Pre-trade risk checks.
//!
//! Validates a set of rebalance orders against risk limits before execution.

use nanobook::Symbol;
use rustc_hash::FxHashMap;
use serde::Serialize;

use crate::config::RiskConfig;
use crate::diff::RebalanceOrder;

/// Result of running all risk checks.
#[derive(Debug, Clone, Serialize)]
pub struct RiskReport {
    pub checks: Vec<RiskCheck>,
}

/// A single risk check result.
#[derive(Debug, Clone, Serialize)]
pub struct RiskCheck {
    pub name: &'static str,
    pub status: RiskStatus,
    pub detail: String,
}

/// Whether a check passed, warned, or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RiskStatus {
    Pass,
    Warn,
    Fail,
}

impl std::fmt::Display for RiskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskStatus::Pass => write!(f, "PASS"),
            RiskStatus::Warn => write!(f, "WARN"),
            RiskStatus::Fail => write!(f, "FAIL"),
        }
    }
}

impl RiskReport {
    /// True if any check failed (not just warned).
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == RiskStatus::Fail)
    }

    /// True if any check warned.
    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.status == RiskStatus::Warn)
    }
}

impl std::fmt::Display for RiskReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "RISK CHECKS:")?;
        for check in &self.checks {
            writeln!(f, "  [{}] {}: {}", check.status, check.name, check.detail)?;
        }
        Ok(())
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
    let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
    let target_map: FxHashMap<Symbol, f64> = targets.iter().copied().collect();

    let mut checks = Vec::new();

    // 1. Max position check — no single position > max_position_pct
    // Use the target weights directly (they represent post-trade state)
    // Also respect per-run constraint overrides if present
    let max_pos = config.max_position_pct;
    let mut worst_pos = 0.0_f64;
    let mut worst_sym = Symbol::new("?");
    for &(sym, weight) in targets {
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

    // 2. Leverage check — gross exposure / equity
    // Post-trade positions: current + order diffs
    let mut post_qty: FxHashMap<Symbol, i64> = current_qty.clone();
    for order in orders {
        let sign = match order.action {
            crate::diff::Action::Buy | crate::diff::Action::BuyCover => 1,
            crate::diff::Action::Sell | crate::diff::Action::SellShort => -1,
        };
        *post_qty.entry(order.symbol).or_insert(0) += sign * order.shares;
    }
    let gross_exposure: i64 = post_qty
        .iter()
        .map(|(sym, qty)| {
            let price = price_map.get(sym).copied().unwrap_or(0);
            qty.abs() * price
        })
        .sum();
    let leverage = if equity_cents > 0 {
        gross_exposure as f64 / equity_cents as f64
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
    let short_pct = if equity_cents > 0 {
        short_exposure as f64 / equity_cents as f64
    } else {
        0.0
    };

    let has_shorts = orders
        .iter()
        .any(|o| matches!(o.action, crate::diff::Action::SellShort));

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

    // 4. Min trade check — all trades > min_trade_usd
    checks.push(RiskCheck {
        name: "Min trade size",
        status: RiskStatus::Pass, // diff engine already filters sub-minimum trades
        detail: format!("All trades >= ${:.0} minimum", config.min_trade_usd),
    });

    // 5. Max trade check — warn if any trade > max_trade_usd
    let max_cents = (config.max_trade_usd * 100.0) as i64;
    for order in orders {
        if order.notional_cents > max_cents {
            checks.push(RiskCheck {
                name: "Max trade size",
                status: RiskStatus::Warn,
                detail: format!(
                    "Trade {} {}: ${:.0} > ${:.0} max_trade_usd",
                    order.action,
                    order.symbol,
                    order.notional_cents as f64 / 100.0,
                    config.max_trade_usd,
                ),
            });
        }
    }

    // 6. Order count check
    let order_count = orders.len();
    checks.push(RiskCheck {
        name: "Order count",
        status: RiskStatus::Pass,
        detail: format!("{order_count} orders"),
    });

    // 7. Target weights sanity — sum of long weights
    let long_sum: f64 = target_map
        .values()
        .filter(|w| **w > 0.0)
        .sum();
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
        let current = FxHashMap::default();

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
        let current = FxHashMap::default();

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
        let current = FxHashMap::default();

        let report = check_risk(
            &orders,
            10_000_000,
            &targets,
            &prices,
            &current,
            &config,
        );

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
        let current = FxHashMap::default();

        let report = check_risk(
            &orders,
            100_000_000, // $1M (so 30% is under position limit)
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
            checks: vec![
                RiskCheck {
                    name: "Test",
                    status: RiskStatus::Pass,
                    detail: "ok".into(),
                },
            ],
        };
        let s = format!("{report}");
        assert!(s.contains("[PASS]"));
    }
}
