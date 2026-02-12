//! Pre-trade risk engine for nanobook.
//!
//! Validates orders against configurable risk limits before execution.
//! Uses generic broker types so it can be called from Python or any broker adapter.

pub mod checks;
pub mod config;
pub mod report;

pub use config::RiskConfig;
pub use report::{RiskCheck, RiskReport, RiskStatus};

use nanobook::Symbol;
use nanobook_broker::{Account, BrokerSide};

/// Pre-trade risk engine.
#[derive(Debug, Clone)]
pub struct RiskEngine {
    config: RiskConfig,
}

impl RiskEngine {
    /// Create a new risk engine with the given config.
    ///
    /// # Panics
    ///
    /// Panics if `config` fails validation (e.g., NaN fields, out-of-range values).
    /// This is intentional — fail-fast at construction, not at check time.
    #[track_caller]
    pub fn new(config: RiskConfig) -> Self {
        if let Err(msg) = config.validate() {
            panic!("invalid RiskConfig: {msg}");
        }
        Self { config }
    }

    /// Access the current config.
    pub fn config(&self) -> &RiskConfig {
        &self.config
    }

    /// Check a single order against risk limits.
    ///
    /// A lightweight check for one order — validates position concentration
    /// and order size.
    pub fn check_order(
        &self,
        symbol: &Symbol,
        side: BrokerSide,
        quantity: u64,
        price_cents: i64,
        account: &Account,
        current_positions: &[(Symbol, i64)],
    ) -> RiskReport {
        let equity = account.equity_cents;
        let notional = quantity as i64 * price_cents;

        let mut checks = Vec::new();

        let max_order = self.config.max_order_value_cents;
        let order_status = if max_order > 0 && notional > max_order {
            RiskStatus::Fail
        } else {
            RiskStatus::Pass
        };
        checks.push(RiskCheck {
            name: "Max order value",
            status: order_status,
            detail: format!(
                "${:.0} {} ${:.0} max_order_value_cents",
                notional as f64 / 100.0,
                if order_status == RiskStatus::Pass {
                    "<="
                } else {
                    ">"
                },
                max_order as f64 / 100.0,
            ),
        });

        // Position concentration after this order
        let current_qty = current_positions
            .iter()
            .find(|(s, _)| s == symbol)
            .map(|(_, q)| *q)
            .unwrap_or(0);

        let delta = match side {
            BrokerSide::Buy => quantity as i64,
            BrokerSide::Sell => -(quantity as i64),
        };
        let post_qty = current_qty + delta;
        let post_value = post_qty.abs() * price_cents;
        let post_pct = if equity > 0 {
            post_value as f64 / equity as f64
        } else {
            0.0
        };

        let pos_status = if post_pct > self.config.max_position_pct {
            RiskStatus::Fail
        } else {
            RiskStatus::Pass
        };
        checks.push(RiskCheck {
            name: "Max position",
            status: pos_status,
            detail: format!(
                "{:.1}% ({}) {} {:.1}% limit",
                post_pct * 100.0,
                symbol.as_str(),
                if pos_status == RiskStatus::Pass {
                    "<="
                } else {
                    ">"
                },
                self.config.max_position_pct * 100.0,
            ),
        });

        // Order size check
        let max_cents = (self.config.max_trade_usd * 100.0) as i64;
        let order_size_status = if notional > max_cents {
            RiskStatus::Warn
        } else {
            RiskStatus::Pass
        };
        checks.push(RiskCheck {
            name: "Order size",
            status: order_size_status,
            detail: format!(
                "${:.2} {} ${:.2} max",
                notional as f64 / 100.0,
                if order_size_status == RiskStatus::Pass {
                    "<="
                } else {
                    ">"
                },
                self.config.max_trade_usd,
            ),
        });

        // Short check
        if side == BrokerSide::Sell && post_qty < 0 && !self.config.allow_short {
            checks.push(RiskCheck {
                name: "Short selling",
                status: RiskStatus::Fail,
                detail: "short selling not allowed".into(),
            });
        }

        RiskReport { checks }
    }

    /// Check a batch of orders (e.g., a full rebalance).
    ///
    /// Validates all risk limits including leverage, short exposure, and
    /// aggregate position limits.
    pub fn check_batch(
        &self,
        orders: &[(Symbol, BrokerSide, u64, i64)], // (symbol, side, qty, price_cents)
        account: &Account,
        current_positions: &[(Symbol, i64)], // (symbol, current_qty)
        target_weights: &[(Symbol, f64)],
    ) -> RiskReport {
        checks::check_batch(
            &self.config,
            orders,
            account,
            current_positions,
            target_weights,
        )
    }
}
