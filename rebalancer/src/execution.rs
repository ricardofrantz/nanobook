//! Execution orchestrator: diff → confirm → execute → reconcile.
//!
//! This is the main workflow that ties together all components.

use std::time::Duration;

use log::{error, info, warn};
use nanobook::Symbol;
use nanobook_broker::ibkr::orders::{self, OrderOutcome};
use nanobook_broker::types::Position;
use nanobook_broker::{BrokerSide, ClientOrderId};
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
    pub cron_mode: bool,
}

/// Cron mode state for idempotency tracking.
///
/// When cron mode is enabled, each rebalance run is assigned a sequence number
/// (Unix timestamp in seconds) that is written to the audit log. The audit log
/// is checked before execution to prevent double-firing the same rebalance window.
#[derive(Debug, Clone)]
pub struct CronMode {
    /// Sequence number for this rebalance run (Unix timestamp in seconds).
    pub sequence_number: u64,
}

impl CronMode {
    /// Create a new CronMode instance with a sequence number.
    ///
    /// The sequence number is typically a Unix timestamp in seconds, ensuring
    /// monotonic increasing values across runs.
    pub fn new(sequence_number: u64) -> Self {
        Self { sequence_number }
    }

    /// Check if cron mode is enabled.
    ///
    /// This method always returns true when CronMode is present, as the presence
    /// of this struct indicates cron mode is active.
    pub fn is_enabled(&self) -> bool {
        true
    }
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

/// Derive the stable broker-side idempotency key for a computed rebalance order.
pub fn derive_client_order_id(
    target: &TargetSpec,
    order: &RebalanceOrder,
) -> Result<ClientOrderId> {
    let side = action_to_side(order.action);
    let qty = u64::try_from(order.shares)
        .map_err(|_| Error::Order(format!("invalid share quantity for order {order:?}")))?;
    let scope = target.idempotency_scope();

    Ok(ClientOrderId::derive(
        &scope,
        order.symbol.as_str(),
        side,
        qty,
    ))
}

/// Execute a full rebalance run.
pub fn run(config: &Config, target: &TargetSpec, opts: &RunOptions) -> Result<()> {
    // In cron mode, check idempotency before connecting
    if opts.cron_mode {
        let window_id = target.window_id();
        // Open audit log to check for previous completion
        let mut audit_check = AuditLog::open(&config.audit_path())?;
        if let Some(existing_seq) = audit_check.check_window_already_complete(&window_id)? {
            warn!(
                "Rebalance window {} already completed with sequence number {} — refusing to run",
                window_id, existing_seq
            );
            // Log the rejection
            let mut audit = AuditLog::open(&config.audit_path())?;
            audit::log_idempotency_rejection(&mut audit, &window_id, existing_seq)?;
            return Err(Error::IdempotencyRejection {
                window_id,
                sequence_number: existing_seq,
            });
        }
    }

    // 1. Connect to IBKR
    let client = connect_ibkr(config)?;

    // 2. Open audit log
    let mut audit = AuditLog::open(&config.audit_path())?;
    audit::log_run_started(&mut audit, &opts.target_file, &config.account.id)?;

    // In cron mode, log the start with sequence number
    let cron_mode = if opts.cron_mode {
        let window_id = target.window_id();
        // Use a simple sequence number based on timestamp (in production, this could be from a config)
        let sequence_number = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| Error::Config(format!("clock error: {e}")))?
            .as_secs();
        audit::log_cron_start(&mut audit, sequence_number, &window_id)?;
        Some(CronMode::new(sequence_number))
    } else {
        None
    };

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

    // 5. Fetch live quotes for all symbols (current + target) and check staleness
    //
    // Staleness detection is critical for trading safety:
    // - Market data can become stale due to network issues, exchange outages, or data feed problems
    // - Trading on stale prices can lead to significant slippage or execution at unfavorable prices
    // - A 30-second old quote may no longer reflect current market conditions, especially during volatility
    // - This check prevents the rebalancer from making decisions based on outdated information
    let all_symbols = collect_all_symbols(&positions, target);

    // Fetch quotes and check staleness
    let quotes = as_connection_error(client.quotes(&all_symbols))?;
    for quote in &quotes {
        if quote.is_stale(config.execution.quote_staleness_threshold_sec) {
            let age_sec = quote.timestamp.elapsed().unwrap_or_default().as_secs();
            return Err(Error::StaleQuote {
                symbol: quote.symbol.as_str().to_string(),
                age_sec,
                threshold_sec: config.execution.quote_staleness_threshold_sec,
            });
        }
    }

    // Extract mid prices from quotes for diff computation
    let prices: Vec<(Symbol, i64)> = quotes
        .iter()
        .map(|q| {
            let mid = match (q.bid_cents, q.ask_cents) {
                (b, a) if b > 0 && a > 0 => b + (a - b) / 2,
                (b, _) if b > 0 => b,
                (_, a) if a > 0 => a,
                _ => q.last_cents,
            };
            (q.symbol, mid)
        })
        .collect();

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
        let client_order_id = derive_client_order_id(target, order)?;

        match client.execute_limit_order(
            order.symbol,
            side,
            shares,
            order.limit_price_cents,
            Some(&client_order_id),
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
    if let Some(cron) = cron_mode {
        let window_id = target.window_id();
        audit::log_cron_completed(&mut audit, cron.sequence_number, &window_id, submitted, filled, failed)?;
    } else {
        audit::log_run_completed(&mut audit, submitted, filled, failed)?;
    }
    println!(
        "\n{submitted} submitted, {filled} filled, {failed} failed. Audit logged to {}",
        config.audit_path().display()
    );

    // 13. Reconcile
    info!("Running post-execution reconciliation...");
    let final_broker_positions = as_connection_error(client.positions())?;
    let final_positions = to_current_positions(&final_broker_positions);

    // Fetch final quotes and extract mid prices
    let final_quotes = as_connection_error(client.quotes(&all_symbols))?;
    let final_prices: Vec<(Symbol, i64)> = final_quotes
        .iter()
        .map(|q| {
            let mid = match (q.bid_cents, q.ask_cents) {
                (b, a) if b > 0 && a > 0 => b + (a - b) / 2,
                (b, _) if b > 0 => b,
                (_, a) if a > 0 => a,
                _ => q.last_cents,
            };
            (q.symbol, mid)
        })
        .collect();

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

    // Fetch quotes and extract mid prices
    let quotes = as_connection_error(client.quotes(&all_symbols))?;
    let prices: Vec<(Symbol, i64)> = quotes
        .iter()
        .map(|q| {
            let mid = match (q.bid_cents, q.ask_cents) {
                (b, a) if b > 0 && a > 0 => b + (a - b) / 2,
                (b, _) if b > 0 => b,
                (_, a) if a > 0 => a,
                _ => q.last_cents,
            };
            (q.symbol, mid)
        })
        .collect();

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
