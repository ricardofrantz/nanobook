//! Execution orchestrator: diff → confirm → execute → reconcile.
//!
//! This is the main workflow that ties together all components.

use std::time::Duration;

use log::{error, info, warn};
use nanobook::Symbol;
use nanobook_broker::BrokerSide;
use nanobook_broker::ibkr::orders::{self, OrderOutcome};
use nanobook_broker::types::Position;
use rustc_hash::FxHashMap;

use crate::audit::{self, AuditLog};
use crate::broker::{as_connection_error, connect_ibkr};
use crate::config::Config;
use crate::diff::{self, Action, CurrentPosition, RebalanceOrder};
use crate::error::{Error, Result};
use crate::reconcile;
use crate::risk;
use crate::target::TargetSpec;

/// Options for a rebalance run.
pub struct RunOptions {
    pub dry_run: bool,
    pub force: bool,
    pub target_file: String,
}

/// Convert broker positions to rebalancer CurrentPosition type.
fn to_current_positions(broker_positions: &[Position]) -> Vec<CurrentPosition> {
    broker_positions
        .iter()
        .map(|p| CurrentPosition {
            symbol: p.symbol,
            quantity: p.quantity,
            avg_cost_cents: p.avg_cost_cents,
        })
        .collect()
}

/// Map a RebalanceOrder action to a BrokerSide.
pub fn action_to_side(action: Action) -> BrokerSide {
    match action {
        Action::Buy | Action::BuyCover => BrokerSide::Buy,
        Action::Sell | Action::SellShort => BrokerSide::Sell,
    }
}

pub fn enforce_max_orders_per_run(
    generated_orders: usize,
    max_orders_per_run: usize,
) -> Result<()> {
    if generated_orders > max_orders_per_run {
        return Err(Error::RiskFailed(format!(
            "{generated_orders} orders generated, but max_orders_per_run is {max_orders_per_run}",
        )));
    }

    Ok(())
}

/// Execute a full rebalance run.
pub fn run(config: &Config, target: &TargetSpec, opts: &RunOptions) -> Result<()> {
    // 1. Connect to IBKR
    let client = connect_ibkr(config)?;

    // 2. Open audit log
    let mut audit = AuditLog::open(&config.audit_path())?;
    audit::log_run_started(&mut audit, &opts.target_file, &config.account.id)?;

    // 3. Fetch account summary
    let summary = as_connection_error(client.account_summary())?;
    println!(
        "Account {} ({}): ${:.2} equity, ${:.2} cash",
        config.account.id,
        format!("{:?}", config.account.account_type).to_lowercase(),
        summary.equity_cents as f64 / 100.0,
        summary.cash_cents as f64 / 100.0,
    );

    // 4. Fetch current positions (convert from broker types to rebalancer types)
    let broker_positions = as_connection_error(client.positions())?;
    let positions = to_current_positions(&broker_positions);
    audit::log_positions(&mut audit, &positions, summary.equity_cents)?;

    display_current_positions(&positions, summary.equity_cents);

    // 5. Fetch live prices for all symbols (current + target)
    let all_symbols = collect_all_symbols(&positions, target);
    let prices = as_connection_error(client.prices(&all_symbols))?;

    // 6. Compute diff
    let targets = target.as_target_pairs();
    let min_trade_cents = (config.risk.min_trade_usd * 100.0) as i64;

    let orders = diff::compute_diff(
        summary.equity_cents,
        &positions,
        &targets,
        &prices,
        config.execution.limit_offset_bps,
        min_trade_cents,
    );

    enforce_max_orders_per_run(orders.len(), config.execution.max_orders_per_run)?;

    if orders.is_empty() {
        println!("\nNo rebalancing needed — portfolio matches target.");
        audit.log_simple("no_rebalance_needed")?;
        return Ok(());
    }

    audit::log_diff(&mut audit, &orders)?;

    // 7. Display the plan
    display_plan(&orders, &config.cost);
    println!();

    // 8. Run risk checks
    let current_qty: FxHashMap<Symbol, i64> =
        positions.iter().map(|p| (p.symbol, p.quantity)).collect();

    let risk_config = apply_constraint_overrides(&config.risk, target);
    let risk_report = risk::check_risk(
        &orders,
        summary.equity_cents,
        &targets,
        &prices,
        &current_qty,
        &risk_config,
    );

    print!("{risk_report}");
    audit::log_risk_check(&mut audit, &risk_report)?;

    if risk_report.has_failures() {
        return Err(Error::RiskFailed(
            "one or more risk checks failed — aborting".into(),
        ));
    }

    // 9. Dry run stops here
    if opts.dry_run {
        println!("\n[DRY RUN] No orders submitted.");
        return Ok(());
    }

    // 10. Confirm execution
    if !opts.force {
        let confirmed = dialoguer::Confirm::new()
            .with_prompt("Execute?")
            .default(false)
            .interact()
            .map_err(|e| Error::Aborted(format!("confirmation prompt failed: {e}")))?;

        if !confirmed {
            println!("Aborted.");
            audit.log("user_confirmed", serde_json::json!({"approved": false}))?;
            return Ok(());
        }

        audit.log("user_confirmed", serde_json::json!({"approved": true}))?;
    }

    // 11. Execute orders
    let timeout = Duration::from_secs(config.execution.order_timeout_secs);
    let mut submitted = 0;
    let mut filled = 0;
    let mut failed = 0;

    for (i, order) in orders.iter().enumerate() {
        print!(
            "[{}/{}] {} {} {} @ ${:.2} ... ",
            i + 1,
            orders.len(),
            order.action,
            order.shares,
            order.symbol,
            order.limit_price_cents as f64 / 100.0,
        );

        submitted += 1;

        let side = action_to_side(order.action);
        let shares = u64::try_from(order.shares)
            .map_err(|_| Error::Order(format!("invalid share quantity for order {order:?}")))?;

        match client.execute_limit_order(
            order.symbol,
            side,
            shares,
            order.limit_price_cents,
            timeout,
        ) {
            Ok(result) => {
                audit::log_order_submitted(&mut audit, order, result.order_id)?;
                audit::log_order_filled(&mut audit, &result)?;

                match result.status {
                    OrderOutcome::Filled => {
                        println!(
                            "FILLED {} @ ${:.2} avg",
                            result.filled_shares, result.avg_fill_price
                        );
                        filled += 1;
                    }
                    OrderOutcome::PartialFill => {
                        println!(
                            "PARTIAL {}/{} @ ${:.2} avg",
                            result.filled_shares, order.shares, result.avg_fill_price
                        );
                        warn!(
                            "Partial fill for {}: {}/{}",
                            order.symbol, result.filled_shares, order.shares
                        );
                        filled += 1; // count as filled (partially)
                    }
                    OrderOutcome::Cancelled => {
                        println!("CANCELLED");
                        failed += 1;
                    }
                    OrderOutcome::Failed => {
                        println!("FAILED");
                        failed += 1;
                    }
                }
            }
            Err(e) => {
                println!("ERROR: {e}");
                error!("Order execution failed for {}: {e}", order.symbol);
                failed += 1;
            }
        }

        // Rate limiting between orders
        if i + 1 < orders.len() {
            orders::rate_limit_delay(config.execution.order_interval_ms);
        }
    }

    // 12. Log completion
    audit::log_run_completed(&mut audit, submitted, filled, failed)?;
    println!(
        "\n{submitted} submitted, {filled} filled, {failed} failed. Audit logged to {}",
        config.audit_path().display()
    );

    // 13. Reconcile
    info!("Running post-execution reconciliation...");
    let final_broker_positions = as_connection_error(client.positions())?;
    let final_positions = to_current_positions(&final_broker_positions);
    let final_prices = as_connection_error(client.prices(&all_symbols))?;
    let final_summary = as_connection_error(client.account_summary())?;

    let report = reconcile::reconcile(
        &final_positions,
        &targets,
        &final_prices,
        final_summary.equity_cents,
    );
    print!("\n{report}");

    Ok(())
}

/// Show current IBKR positions.
pub fn show_positions(config: &Config) -> Result<()> {
    let client = connect_ibkr(config)?;
    let summary = as_connection_error(client.account_summary())?;
    let broker_positions = as_connection_error(client.positions())?;
    let positions = to_current_positions(&broker_positions);

    println!(
        "Account {} ({}): ${:.2} equity, ${:.2} cash\n",
        config.account.id,
        format!("{:?}", config.account.account_type).to_lowercase(),
        summary.equity_cents as f64 / 100.0,
        summary.cash_cents as f64 / 100.0,
    );

    display_current_positions(&positions, summary.equity_cents);
    Ok(())
}

/// Check IBKR connection status.
pub fn check_status(config: &Config) -> Result<()> {
    print!(
        "Connecting to IB Gateway at {}:{}... ",
        config.connection.host, config.connection.port
    );

    let client = connect_ibkr(config)?;
    println!("OK");

    let summary = as_connection_error(client.account_summary())?;
    println!(
        "Account {}: ${:.2} equity",
        config.account.id,
        summary.equity_cents as f64 / 100.0,
    );

    Ok(())
}

/// Run reconciliation against the last target.
pub fn run_reconcile(config: &Config, target: &TargetSpec) -> Result<()> {
    let client = connect_ibkr(config)?;
    let summary = as_connection_error(client.account_summary())?;
    let broker_positions = as_connection_error(client.positions())?;
    let positions = to_current_positions(&broker_positions);

    let all_symbols = collect_all_symbols(&positions, target);
    let prices = as_connection_error(client.prices(&all_symbols))?;
    let targets = target.as_target_pairs();

    let report = reconcile::reconcile(&positions, &targets, &prices, summary.equity_cents);
    print!("{report}");

    Ok(())
}

// === Helpers ===

pub fn collect_all_symbols(positions: &[CurrentPosition], target: &TargetSpec) -> Vec<Symbol> {
    let mut symbols: Vec<Symbol> = positions.iter().map(|p| p.symbol).collect();
    for sym in target.symbols() {
        if !symbols.contains(&sym) {
            symbols.push(sym);
        }
    }
    symbols
}

fn display_current_positions(positions: &[CurrentPosition], equity_cents: i64) {
    if positions.is_empty() {
        println!("No positions.");
        return;
    }

    println!("CURRENT PORTFOLIO:");
    for pos in positions {
        let weight = if equity_cents > 0 {
            // Approximate — uses avg cost as price proxy (actual price may differ)
            pos.quantity as f64 * pos.avg_cost_cents as f64 / equity_cents as f64
        } else {
            0.0
        };
        println!(
            "  {:8} {:>6} @ ${:>8.2} avg = ${:>10.2}  ({:.1}%)",
            pos.symbol,
            pos.quantity,
            pos.avg_cost_cents as f64 / 100.0,
            (pos.quantity * pos.avg_cost_cents) as f64 / 100.0,
            weight * 100.0,
        );
    }
}

fn display_plan(orders: &[RebalanceOrder], cost_config: &crate::config::CostConfig) {
    println!("\nREBALANCE ORDERS:");
    println!(
        "  {:>3}  {:10} {:8} {:>8} {:>10} {:>12}",
        "#", "Action", "Symbol", "Shares", "Limit", "Notional"
    );

    for (i, order) in orders.iter().enumerate() {
        println!(
            "  {:>3}  {:10} {:8} {:>8} ${:>9.2} ${:>11.2}   ({})",
            i + 1,
            format!("{}", order.action),
            order.symbol,
            order.shares,
            order.limit_price_cents as f64 / 100.0,
            order.notional_cents as f64 / 100.0,
            order.description,
        );
    }

    let cost = diff::estimate_cost(
        orders,
        cost_config.commission_per_share,
        cost_config.commission_min,
        cost_config.slippage_bps,
    );
    println!("\nEst. cost: {cost}");
}

pub fn apply_constraint_overrides(
    base: &crate::config::RiskConfig,
    target: &TargetSpec,
) -> crate::config::RiskConfig {
    let mut config = base.clone();
    if let Some(ref constraints) = target.constraints {
        if let Some(max_pos) = constraints.max_position_pct {
            config.max_position_pct = max_pos;
        }
        if let Some(max_lev) = constraints.max_leverage {
            config.max_leverage = max_lev;
        }
        if let Some(min_trade) = constraints.min_trade_usd {
            config.min_trade_usd = min_trade;
        }
    }
    config
}
