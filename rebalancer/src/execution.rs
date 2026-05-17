//! Execution orchestrator: diff → confirm → execute → reconcile.
//!
//! This is the main workflow that ties together all components.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::time::{Duration, Instant};

use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::ibkr::orders::{self, OrderOutcome};
use nanobook_broker::types::{Account, Position, Quote};
use nanobook_broker::{BrokerSide, ClientOrderId};
use rustc_hash::{FxHashMap, FxHashSet};
use tracing::{error, info, warn};

use crate::audit::{self, AuditLog};
use crate::broker::{BrokerGateway, as_connection_error, connect_ibkr};
use crate::config::Config;
use crate::diff::{self, Action, CurrentPosition, RebalanceOrder};
use crate::error::{Error, Result};
use crate::observability::generate_correlation_id;
use crate::reconcile;
use crate::risk;
use crate::target::TargetSpec;

#[cfg(feature = "write_ahead_logging")]
use chrono::Utc;

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

fn quote_mid_cents(quote: &Quote) -> i64 {
    match (quote.bid_cents, quote.ask_cents) {
        (bid, ask) if bid > 0 && ask > 0 => bid + (ask - bid) / 2,
        (bid, _) if bid > 0 => bid,
        (_, ask) if ask > 0 => ask,
        _ => quote.last_cents,
    }
}

fn quote_mid_prices(quotes: &[Quote]) -> Vec<(Symbol, i64)> {
    quotes
        .iter()
        .map(|quote| (quote.symbol, quote_mid_cents(quote)))
        .collect()
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

fn next_checkpoint_sequence(sequence_number: &mut u64) -> u64 {
    *sequence_number = (*sequence_number).saturating_add(1);
    *sequence_number
}

/// Process-wide graceful shutdown flag set by SIGTERM.
#[derive(Debug, Clone)]
pub struct ShutdownFlag {
    requested: Arc<AtomicBool>,
}

impl ShutdownFlag {
    pub fn new() -> Self {
        Self {
            requested: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn install_sigterm_handler(&self) -> Result<()> {
        #[cfg(unix)]
        {
            signal_hook::flag::register(signal_hook::consts::SIGTERM, Arc::clone(&self.requested))
                .map_err(|e| Error::Config(format!("failed to install SIGTERM handler: {e}")))?;
        }
        Ok(())
    }

    pub fn request_shutdown(&self) {
        self.requested.store(true, Ordering::SeqCst);
    }

    pub fn is_requested(&self) -> bool {
        self.requested.load(Ordering::SeqCst)
    }
}

impl Default for ShutdownFlag {
    fn default() -> Self {
        Self::new()
    }
}

pub fn remaining_orders_after(current_index: usize, total_orders: usize) -> usize {
    total_orders.saturating_sub(current_index.saturating_add(1))
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
    let correlation_id = generate_correlation_id();
    let run_span = tracing::info_span!(
        "rebalance_run",
        correlation_id = %correlation_id,
        target_file = %opts.target_file,
        account = %config.account.id,
        cron_mode = opts.cron_mode,
        dry_run = opts.dry_run,
    );
    let _run_guard = run_span.enter();

    let shutdown = ShutdownFlag::new();
    shutdown.install_sigterm_handler()?;

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
    let client = {
        let _span = tracing::info_span!(
            "connect_to_broker",
            host = %config.connection.host,
            port = config.connection.port,
            client_id = config.connection.client_id,
        )
        .entered();
        connect_ibkr(config)?
    };

    // 2. Open audit log
    let audit_path = config.audit_path();
    let existing_checkpoint_sequence = audit::max_checkpoint_sequence(&audit_path)?;
    let mut audit = AuditLog::open(&audit_path)?;
    let run_sequence_number = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| Error::Config(format!("clock error: {e}")))?
        .as_secs()
        .max(existing_checkpoint_sequence);
    let mut checkpoint_sequence = run_sequence_number;
    let target_spec_ref = target.idempotency_scope();

    #[cfg(feature = "write_ahead_logging")]
    audit::log_run_started_checkpoint(
        &mut audit,
        next_checkpoint_sequence(&mut checkpoint_sequence),
        &opts.target_file,
        &config.account.id,
    )?;
    #[cfg(not(feature = "write_ahead_logging"))]
    audit::log_run_started(&mut audit, &opts.target_file, &config.account.id)?;

    // In cron mode, log the start with sequence number
    let cron_mode = if opts.cron_mode {
        let window_id = target.window_id();
        audit::log_cron_start(&mut audit, run_sequence_number, &window_id)?;
        Some(CronMode::new(run_sequence_number))
    } else {
        None
    };

    // 3. Fetch account summary
    let summary = {
        let _span =
            tracing::info_span!("fetch_account_summary", account = %config.account.id).entered();
        fetch_account_summary_with_write_ahead(
            client.as_ref(),
            &mut audit,
            &mut checkpoint_sequence,
            &target_spec_ref,
        )?
    };
    println!(
        "Account {} ({}): ${:.2} equity, ${:.2} cash",
        config.account.id,
        format!("{:?}", config.account.account_type).to_lowercase(),
        summary.equity_cents as f64 / 100.0,
        summary.cash_cents as f64 / 100.0,
    );

    // 4. Fetch current positions (convert from broker types to rebalancer types)
    let broker_positions = {
        let _span =
            tracing::info_span!("fetch_positions", equity_cents = summary.equity_cents).entered();
        fetch_positions_with_write_ahead(
            client.as_ref(),
            &mut audit,
            &mut checkpoint_sequence,
            &target_spec_ref,
            summary.equity_cents,
        )?
    };
    let positions = to_current_positions(&broker_positions);

    #[cfg(not(feature = "write_ahead_logging"))]
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
    let quotes = {
        let _span = tracing::info_span!("fetch_quotes", symbol_count = all_symbols.len()).entered();
        fetch_quotes_with_write_ahead(
            client.as_ref(),
            &mut audit,
            &mut checkpoint_sequence,
            &target_spec_ref,
            &all_symbols,
            config.execution.quote_staleness_threshold_sec,
        )?
    };
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
    let prices = quote_mid_prices(&quotes);

    // 6. Compute diff
    let targets = target.as_target_pairs();
    let min_trade_cents = (config.risk.min_trade_usd * 100.0) as i64;

    let orders = {
        let _span = tracing::info_span!(
            "compute_diff",
            position_count = positions.len(),
            target_count = targets.len(),
            quote_count = prices.len(),
            equity_cents = summary.equity_cents,
        )
        .entered();
        diff::compute_diff(
            summary.equity_cents,
            &positions,
            &targets,
            &prices,
            config.execution.limit_offset_bps,
            min_trade_cents,
        )
    };

    enforce_max_orders_per_run(orders.len(), config.execution.max_orders_per_run)?;

    #[cfg(feature = "write_ahead_logging")]
    audit::log_diff_checkpoint(
        &mut audit,
        next_checkpoint_sequence(&mut checkpoint_sequence),
        &orders,
    )?;
    #[cfg(not(feature = "write_ahead_logging"))]
    audit::log_diff(&mut audit, &orders)?;

    if orders.is_empty() {
        println!("\nNo rebalancing needed — portfolio matches target.");
        audit.log_simple("no_rebalance_needed")?;
        #[cfg(feature = "write_ahead_logging")]
        audit::log_run_completed_checkpoint(
            &mut audit,
            next_checkpoint_sequence(&mut checkpoint_sequence),
            0,
            0,
            0,
        )?;
        #[cfg(not(feature = "write_ahead_logging"))]
        audit::log_run_completed(&mut audit, 0, 0, 0)?;
        return Ok(());
    }

    // 7. Display the plan
    display_plan(&orders, &config.cost);
    println!();

    // 8. Run risk checks
    let current_qty: FxHashMap<Symbol, i64> =
        positions.iter().map(|p| (p.symbol, p.quantity)).collect();

    let risk_config = apply_constraint_overrides(&config.risk, target);
    let risk_report = {
        let _span = tracing::info_span!("risk_check", order_count = orders.len()).entered();
        risk::check_risk(
            &orders,
            summary.equity_cents,
            &targets,
            &prices,
            &current_qty,
            &risk_config,
        )
    };

    print!("{risk_report}");
    audit::log_risk_check(&mut audit, &risk_report)?;

    if risk_report.has_failures() {
        return Err(Error::RiskFailed(
            "one or more risk checks failed — aborting".into(),
        ));
    }

    #[cfg(feature = "write_ahead_logging")]
    audit::log_risk_check_passed_checkpoint(
        &mut audit,
        next_checkpoint_sequence(&mut checkpoint_sequence),
        &risk_report,
    )?;

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
    let execute_span = tracing::info_span!("execute_orders", total_orders = orders.len());
    let _execute_guard = execute_span.enter();
    let mut submitted = 0;
    let mut filled = 0;
    let mut failed = 0;
    let shutdown_started_at = Instant::now();

    for (i, order) in orders.iter().enumerate() {
        if shutdown.is_requested() {
            let cancelled = orders.len().saturating_sub(i);
            warn!(
                "Graceful shutdown requested before order {}; skipping {} queued orders",
                i + 1,
                cancelled
            );
            audit::log_kill_completed(
                &mut audit,
                "graceful",
                cancelled,
                shutdown_started_at.elapsed().as_secs_f64(),
            )?;
            return Ok(());
        }

        let _order_span = tracing::info_span!(
            "submit_order",
            order_index = i + 1,
            total_orders = orders.len(),
            symbol = %order.symbol,
            action = %order.action,
            shares = order.shares,
            limit_price_cents = order.limit_price_cents,
        )
        .entered();

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

        let client_order_id = derive_client_order_id(target, order)?;
        let order_sequence = next_checkpoint_sequence(&mut checkpoint_sequence);

        let execution_result = execute_order_with_write_ahead(
            client.as_ref(),
            &mut audit,
            order,
            &client_order_id,
            timeout,
            order_sequence,
            &target_spec_ref,
        );
        checkpoint_sequence = checkpoint_sequence.max(order_sequence.saturating_add(1));
        let mut current_order_to_cancel = None;

        match execution_result {
            Ok(result) => {
                #[cfg(feature = "write_ahead_logging")]
                audit::log_order_filled_checkpoint(
                    &mut audit,
                    next_checkpoint_sequence(&mut checkpoint_sequence),
                    &result,
                )?;
                #[cfg(not(feature = "write_ahead_logging"))]
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
                        current_order_to_cancel = Some(result.order_id);
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

        if shutdown.is_requested() {
            let mut cancelled = remaining_orders_after(i, orders.len());
            if let Some(order_id) = current_order_to_cancel {
                cancel_order_with_write_ahead(
                    client.as_ref(),
                    &mut audit,
                    &mut checkpoint_sequence,
                    u64::try_from(order_id).unwrap_or_default(),
                    "graceful_shutdown_partial_fill",
                )?;
                cancelled += 1;
            }
            warn!(
                "Graceful shutdown requested after order {}; cancelling/skipping {} remaining orders",
                i + 1,
                cancelled
            );
            audit::log_kill_completed(
                &mut audit,
                "graceful",
                cancelled,
                shutdown_started_at.elapsed().as_secs_f64(),
            )?;
            return Ok(());
        }

        // Rate limiting between orders
        if i + 1 < orders.len() {
            orders::rate_limit_delay(config.execution.order_interval_ms);
        }
    }

    drop(_execute_guard);

    // 12. Reconcile
    let _reconcile_span = tracing::info_span!("reconcile", submitted, filled, failed).entered();
    info!("Running post-execution reconciliation...");
    let final_broker_positions = fetch_positions_with_write_ahead(
        client.as_ref(),
        &mut audit,
        &mut checkpoint_sequence,
        &target_spec_ref,
        summary.equity_cents,
    )?;
    let final_positions = to_current_positions(&final_broker_positions);

    // Fetch final quotes and extract mid prices
    let final_quotes = fetch_quotes_with_write_ahead(
        client.as_ref(),
        &mut audit,
        &mut checkpoint_sequence,
        &target_spec_ref,
        &all_symbols,
        config.execution.quote_staleness_threshold_sec,
    )?;
    let final_prices = quote_mid_prices(&final_quotes);

    let final_summary = fetch_account_summary_with_write_ahead(
        client.as_ref(),
        &mut audit,
        &mut checkpoint_sequence,
        &target_spec_ref,
    )?;

    let report = reconcile::reconcile(
        &final_positions,
        &targets,
        &final_prices,
        final_summary.equity_cents,
    );
    print!("\n{report}");

    // 13. Log completion after post-execution reconciliation so the final
    // checkpoint means all broker-observed state was captured.
    if let Some(cron) = cron_mode {
        let window_id = target.window_id();
        audit::log_cron_completed(
            &mut audit,
            cron.sequence_number,
            &window_id,
            submitted,
            filled,
            failed,
        )?;
    }
    #[cfg(feature = "write_ahead_logging")]
    audit::log_run_completed_checkpoint(
        &mut audit,
        next_checkpoint_sequence(&mut checkpoint_sequence),
        submitted,
        filled,
        failed,
    )?;
    #[cfg(not(feature = "write_ahead_logging"))]
    audit::log_run_completed(&mut audit, submitted, filled, failed)?;

    println!(
        "\n{submitted} submitted, {filled} filled, {failed} failed. Audit logged to {}",
        config.audit_path().display()
    );

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
    let prices = quote_mid_prices(&quotes);

    let targets = target.as_target_pairs();

    let report = reconcile::reconcile(&positions, &targets, &prices, summary.equity_cents);
    print!("{report}");

    Ok(())
}

// === Helpers ===

pub fn collect_all_symbols(positions: &[CurrentPosition], target: &TargetSpec) -> Vec<Symbol> {
    let target_symbols = target.symbols();
    let mut symbols = Vec::with_capacity(positions.len() + target_symbols.len());
    let mut seen = FxHashSet::default();

    for position in positions {
        symbols.push(position.symbol);
        seen.insert(position.symbol);
    }

    for symbol in target_symbols {
        if seen.insert(symbol) {
            symbols.push(symbol);
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

/// Fetch account summary with write-ahead intent/result logging.
#[cfg(feature = "write_ahead_logging")]
pub fn fetch_account_summary_with_write_ahead(
    broker: &dyn BrokerGateway,
    audit: &mut AuditLog,
    sequence_number: &mut u64,
    target_spec_reference: &str,
) -> Result<Account> {
    audit::log_account_summary_intent_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        Utc::now(),
        target_spec_reference,
    )?;

    let summary = as_connection_error(broker.account_summary())?;

    audit::log_account_summary_result_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        summary.equity_cents,
        summary.cash_cents,
    )?;

    Ok(summary)
}

/// Fetch account summary without write-ahead logging when the feature is disabled.
#[cfg(not(feature = "write_ahead_logging"))]
pub fn fetch_account_summary_with_write_ahead(
    broker: &dyn BrokerGateway,
    _audit: &mut AuditLog,
    _sequence_number: &mut u64,
    _target_spec_reference: &str,
) -> Result<Account> {
    as_connection_error(broker.account_summary())
}

/// Fetch positions with write-ahead intent/result logging.
#[cfg(feature = "write_ahead_logging")]
pub fn fetch_positions_with_write_ahead(
    broker: &dyn BrokerGateway,
    audit: &mut AuditLog,
    sequence_number: &mut u64,
    target_spec_reference: &str,
    equity_cents: i64,
) -> Result<Vec<Position>> {
    audit::log_positions_intent_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        Utc::now(),
        target_spec_reference,
    )?;

    let positions = as_connection_error(broker.positions())?;
    let current_positions = to_current_positions(&positions);

    audit::log_positions_result_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        &current_positions,
        equity_cents,
    )?;

    Ok(positions)
}

/// Fetch positions without write-ahead logging when the feature is disabled.
#[cfg(not(feature = "write_ahead_logging"))]
pub fn fetch_positions_with_write_ahead(
    broker: &dyn BrokerGateway,
    _audit: &mut AuditLog,
    _sequence_number: &mut u64,
    _target_spec_reference: &str,
    _equity_cents: i64,
) -> Result<Vec<Position>> {
    as_connection_error(broker.positions())
}

/// Fetch quotes with write-ahead intent/result logging.
#[cfg(feature = "write_ahead_logging")]
pub fn fetch_quotes_with_write_ahead(
    broker: &dyn BrokerGateway,
    audit: &mut AuditLog,
    sequence_number: &mut u64,
    target_spec_reference: &str,
    symbols: &[Symbol],
    staleness_threshold_sec: u64,
) -> Result<Vec<Quote>> {
    audit::log_quotes_intent_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        symbols,
        staleness_threshold_sec,
        Utc::now(),
        target_spec_reference,
    )?;

    let quotes = as_connection_error(broker.quotes(symbols))?;

    audit::log_quotes_result_checkpoint(audit, next_checkpoint_sequence(sequence_number), &quotes)?;

    Ok(quotes)
}

/// Fetch quotes without write-ahead logging when the feature is disabled.
#[cfg(not(feature = "write_ahead_logging"))]
pub fn fetch_quotes_with_write_ahead(
    broker: &dyn BrokerGateway,
    _audit: &mut AuditLog,
    _sequence_number: &mut u64,
    _target_spec_reference: &str,
    symbols: &[Symbol],
    _staleness_threshold_sec: u64,
) -> Result<Vec<Quote>> {
    as_connection_error(broker.quotes(symbols))
}

/// Cancel an order with write-ahead intent/result logging.
#[cfg(feature = "write_ahead_logging")]
pub fn cancel_order_with_write_ahead(
    broker: &dyn BrokerGateway,
    audit: &mut AuditLog,
    sequence_number: &mut u64,
    order_id: u64,
    cancellation_reason: &str,
) -> Result<()> {
    audit::log_cancel_intent_checkpoint(
        audit,
        next_checkpoint_sequence(sequence_number),
        order_id,
        cancellation_reason,
        Utc::now(),
    )?;

    match broker.cancel_order(order_id) {
        Ok(()) => {
            audit::log_cancel_result_checkpoint(
                audit,
                next_checkpoint_sequence(sequence_number),
                order_id,
                true,
                None,
            )?;
            Ok(())
        }
        Err(error) => {
            audit::log_cancel_result_checkpoint(
                audit,
                next_checkpoint_sequence(sequence_number),
                order_id,
                false,
                Some(&error.to_string()),
            )?;
            Err(Error::Order(format!(
                "cancel failed for order {order_id}: {error}"
            )))
        }
    }
}

/// Cancel an order without write-ahead logging when the feature is disabled.
#[cfg(not(feature = "write_ahead_logging"))]
pub fn cancel_order_with_write_ahead(
    broker: &dyn BrokerGateway,
    _audit: &mut AuditLog,
    _sequence_number: &mut u64,
    order_id: u64,
    _cancellation_reason: &str,
) -> Result<()> {
    broker
        .cancel_order(order_id)
        .map_err(|e| Error::Order(format!("cancel failed for order {order_id}: {e}")))
}

/// Determine if a broker error is transient (retryable) or permanent.
///
/// Transient errors: network timeouts, connection resets, rate limits
/// Permanent errors: invalid symbol, insufficient margin, authentication failures
#[cfg_attr(not(feature = "write_ahead_logging"), allow(dead_code))]
fn is_transient_error(error: &BrokerError) -> bool {
    match error {
        BrokerError::Connection(_) => true,
        BrokerError::NotConnected => true,
        BrokerError::RateLimit => true,
        BrokerError::ConnectionLost { .. } => true,
        BrokerError::ReconnectFailed { .. } => true,
        // Permanent errors
        BrokerError::InvalidSymbol(_) => false,
        BrokerError::Auth(_) => false,
        BrokerError::Order(_) => false, // Most order errors are permanent
        BrokerError::DuplicateOrder { .. } => false,
        BrokerError::CancelReject { .. } => false,
        BrokerError::NonFiniteValue { .. } => false,
        BrokerError::ValueOutOfRange { .. } => false,
        BrokerError::NoQuoteForMarketOrder { .. } => false,
        BrokerError::MarketOrderRejected => false,
        BrokerError::Other(_) => false, // Conservative: treat as permanent
    }
}

/// Execute an order with write-ahead logging and retry logic.
///
/// This function encapsulates the order submission with write-ahead logging:
/// 1. Log OrderIntent checkpoint BEFORE calling the broker
/// 2. Execute the order via the broker
/// 3. Log success (OrderSubmitted) or failure (OrderFailed)
/// 4. Implement retry logic with exponential backoff for transient errors
///
/// # Arguments
///
/// * `broker` - The broker gateway to execute the order
/// * `audit` - The audit log for write-ahead logging
/// * `order` - The rebalance order to execute
/// * `client_order_id` - The client-side idempotency key for the order
/// * `timeout` - Timeout for the order execution
/// * `sequence_number` - Sequence number for checkpoint logging
/// * `target_spec` - Target spec reference for audit context
///
/// # Returns
///
/// * `Ok(OrderResult)` - The order execution result
/// * `Err(Error)` - The error if all retries are exhausted or a permanent error occurs
#[cfg(feature = "write_ahead_logging")]
pub fn execute_order_with_write_ahead(
    broker: &dyn BrokerGateway,
    audit: &mut AuditLog,
    order: &RebalanceOrder,
    client_order_id: &ClientOrderId,
    timeout: Duration,
    sequence_number: u64,
    target_spec: &str,
) -> Result<orders::OrderResult> {
    let side = action_to_side(order.action);
    let shares = u64::try_from(order.shares)
        .map_err(|_| Error::Order(format!("invalid share quantity for order {order:?}")))?;
    let client_order_id_str = client_order_id.as_str();
    let timestamp = Utc::now();
    let execution_context = format!("rebalance_run:{sequence_number}");

    // Log OrderIntent checkpoint BEFORE calling the broker (write-ahead logging)
    audit::log_order_intent_checkpoint(
        audit,
        sequence_number,
        order,
        client_order_id_str,
        timestamp,
        target_spec,
        &execution_context,
    )?;

    // Retry loop with exponential backoff
    let mut attempt = 0;
    let max_retries = 5;

    loop {
        attempt += 1;

        match broker.execute_limit_order(
            order.symbol,
            side,
            shares,
            order.limit_price_cents,
            Some(client_order_id),
            timeout,
        ) {
            Ok(result) => {
                // Log OrderSubmitted checkpoint on success
                audit::log_order_submitted_checkpoint(
                    audit,
                    sequence_number.saturating_add(1),
                    order,
                    result.order_id,
                )?;
                return Ok(result);
            }
            Err(broker_error) => {
                // Check if error is transient (retryable)
                if is_transient_error(&broker_error) {
                    if attempt < max_retries {
                        // Calculate exponential backoff: 1s, 2s, 4s, 8s, 16s
                        let backoff_secs = 2u64.pow(attempt - 1);
                        warn!(
                            "Transient error on attempt {}/{} (will retry in {}s): {}",
                            attempt, max_retries, backoff_secs, broker_error
                        );
                        std::thread::sleep(Duration::from_secs(backoff_secs));
                        continue;
                    } else {
                        // Max retries exceeded - log failure and return error
                        error!(
                            "Max retries ({}) exceeded for order {}: {}",
                            max_retries, order.symbol, broker_error
                        );
                        audit::log_order_failed_checkpoint(
                            audit,
                            sequence_number.saturating_add(1),
                            "max_retries_exceeded",
                            &format!("{} (attempts: {})", broker_error, attempt),
                            &format!(
                                "symbol:{}, client_order_id:{}",
                                order.symbol, client_order_id_str
                            ),
                        )?;
                        return Err(Error::Order(format!(
                            "order failed after {} retries: {}",
                            max_retries, broker_error
                        )));
                    }
                } else {
                    // Permanent error - log failure and return immediately
                    error!(
                        "Permanent error for order {}: {}",
                        order.symbol, broker_error
                    );
                    audit::log_order_failed_checkpoint(
                        audit,
                        sequence_number.saturating_add(1),
                        "permanent_error",
                        &broker_error.to_string(),
                        &format!(
                            "symbol:{}, client_order_id:{}",
                            order.symbol, client_order_id_str
                        ),
                    )?;
                    return Err(Error::Order(format!("order failed: {}", broker_error)));
                }
            }
        }
    }
}

/// Fallback function for when write_ahead_logging feature is disabled.
///
/// This function provides backward compatibility by directly calling the broker
/// without write-ahead logging or retry logic.
#[cfg(not(feature = "write_ahead_logging"))]
pub fn execute_order_with_write_ahead(
    broker: &dyn BrokerGateway,
    _audit: &mut AuditLog,
    order: &RebalanceOrder,
    client_order_id: &ClientOrderId,
    timeout: Duration,
    _sequence_number: u64,
    _target_spec: &str,
) -> Result<orders::OrderResult> {
    let side = action_to_side(order.action);
    let shares = u64::try_from(order.shares)
        .map_err(|_| Error::Order(format!("invalid share quantity for order {order:?}")))?;

    broker
        .execute_limit_order(
            order.symbol,
            side,
            shares,
            order.limit_price_cents,
            Some(client_order_id),
            timeout,
        )
        .map_err(|e| Error::Order(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanobook_broker::error::BrokerError;

    #[test]
    fn test_is_transient_error_connection() {
        assert!(is_transient_error(&BrokerError::Connection(
            "timeout".to_string()
        )));
        assert!(is_transient_error(&BrokerError::NotConnected));
    }

    #[test]
    fn test_is_transient_error_rate_limit() {
        assert!(is_transient_error(&BrokerError::RateLimit));
    }

    #[test]
    fn test_is_transient_error_connection_lost() {
        assert!(is_transient_error(&BrokerError::ConnectionLost {
            order_id: 123,
            filled_quantity: 100
        }));
    }

    #[test]
    fn test_is_transient_error_reconnect_failed() {
        assert!(is_transient_error(&BrokerError::ReconnectFailed {
            attempts: 3,
            reason: "timeout".to_string()
        }));
    }

    #[test]
    fn test_is_not_transient_error_invalid_symbol() {
        assert!(!is_transient_error(&BrokerError::InvalidSymbol(
            "BAD".to_string()
        )));
    }

    #[test]
    fn test_is_not_transient_error_auth() {
        assert!(!is_transient_error(&BrokerError::Auth(
            "unauthorized".to_string()
        )));
    }

    #[test]
    fn test_is_not_transient_error_order() {
        assert!(!is_transient_error(&BrokerError::Order(
            "insufficient margin".to_string()
        )));
    }

    #[test]
    fn test_is_not_transient_error_duplicate_order() {
        assert!(!is_transient_error(&BrokerError::DuplicateOrder {
            client_order_id: "test-123".to_string()
        }));
    }

    #[test]
    fn test_is_not_transient_error_other() {
        assert!(!is_transient_error(&BrokerError::Other(
            "unknown error".to_string()
        )));
    }

    #[test]
    fn test_shutdown_flag_sets_and_reads() {
        let shutdown = ShutdownFlag::new();
        assert!(!shutdown.is_requested());
        shutdown.request_shutdown();
        assert!(shutdown.is_requested());
    }

    #[test]
    fn test_multiple_shutdown_requests_are_idempotent() {
        let shutdown = ShutdownFlag::new();
        shutdown.request_shutdown();
        shutdown.request_shutdown();
        assert!(shutdown.is_requested());
    }

    #[test]
    fn test_remaining_orders_after_current_order() {
        assert_eq!(remaining_orders_after(0, 3), 2);
        assert_eq!(remaining_orders_after(1, 3), 1);
        assert_eq!(remaining_orders_after(2, 3), 0);
        assert_eq!(remaining_orders_after(3, 3), 0);
    }

    #[test]
    fn test_action_to_side_buy() {
        assert_eq!(action_to_side(Action::Buy), BrokerSide::Buy);
        assert_eq!(action_to_side(Action::BuyCover), BrokerSide::Buy);
    }

    #[test]
    fn test_action_to_side_sell() {
        assert_eq!(action_to_side(Action::Sell), BrokerSide::Sell);
        assert_eq!(action_to_side(Action::SellShort), BrokerSide::Sell);
    }

    // Feature flag gating tests

    #[test]
    fn test_cfg_macro_compiles_correctly() {
        // This test verifies that the cfg macro compiles correctly with both feature states
        // The test itself will compile and run in both configurations
        #[cfg(feature = "write_ahead_logging")]
        let feature_enabled = true;
        #[cfg(not(feature = "write_ahead_logging"))]
        let feature_enabled = false;

        // Just verify the test compiles and runs
        let _ = feature_enabled;
    }

    #[test]
    fn test_feature_flag_does_not_affect_other_modules() {
        // Verify that other functions in the module work regardless of feature flag
        // Test action_to_side which is not feature-gated
        assert_eq!(action_to_side(Action::Buy), BrokerSide::Buy);
        assert_eq!(action_to_side(Action::Sell), BrokerSide::Sell);

        // Test enforce_max_orders_per_run which is not feature-gated
        assert!(enforce_max_orders_per_run(3, 5).is_ok());
        assert!(enforce_max_orders_per_run(10, 5).is_err());
    }
}
