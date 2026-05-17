//! Kill switch: send SIGTERM to running runner and verify no dangling orders.
//!
//! # Overview
//!
//! The kill switch provides a safe way to terminate a running rebalancer instance
//! and verify that no orders are left dangling on the exchange. This is important
//! for operational safety, allowing operators to stop automated rebalancing in
//! response to issues or changing market conditions.
//!
//! # PID File
//!
//! The kill switch locates the running rebalancer instance via a PID file at
//! `rebalancer.pid` in the working directory. The PID file contains the process
//! ID of the running rebalancer as a plain text integer.
//!
//! # Order Verification
//!
//! After sending SIGTERM, the kill switch queries the audit log (configured via
//! `logging.dir` and `logging.audit_file` in config.toml) to verify that no orders
//! are left dangling. An order is considered dangling if:
//!
//! - It was submitted (an `order_submitted` event exists in the audit log)
//! - It was not filled (no corresponding `order_filled` event exists)
//!
//! # Manual Intervention
//!
//! If dangling orders are detected, the kill switch will report an error and exit
//! with a non-zero status. This requires manual intervention:
//!
//! 1. Check the exchange directly for any open orders
//! 2. Cancel any orders that should not remain active
//! 3. Investigate why orders were not filled (e.g., market conditions, connectivity issues)
//! 4. Once resolved, remove the PID file manually if needed
//!
//! # Usage
//!
//! ```bash
//! rebalancer --config config.toml kill
//! ```
//!
//! # Error Cases
//!
//! - **PID file not found**: No rebalancer appears to be running. Check if a rebalancer
//!   process exists and whether the PID file was removed manually.
//! - **Process does not exist**: The PID file exists but the process is not running.
//!   This may indicate a crash or manual termination. Remove the PID file and investigate.
//! - **Permission denied**: The current user does not have permission to send signals to
//!   the target process. Ensure the kill command is run with appropriate permissions.
//! - **Dangling orders detected**: Orders were submitted but not filled. Manual intervention
//!   is required to cancel or monitor these orders.
//! - **Timeout**: The process did not exit within the 30-second timeout. This may indicate
//!   the process is hung or not responding to SIGTERM. Consider using SIGKILL as a last resort.

use crate::audit::{AuditLog, log_kill_completed_with_summary, log_kill_requested};
use crate::config::Config;
use crate::error::{Error, Result};
use crate::pid_file::{self, pid_file_exists, read_pid_file};
use nanobook_broker::Broker;
#[cfg(feature = "guaranteed_kill_switch")]
use nanobook_broker::ibkr::IbkrBroker;
use nanobook_broker::types::{BrokerOrderStatus, OrderState};
use serde_json::Value;
use std::path::Path;
use std::thread;
use std::time::Duration;
use tracing::info;

/// Represents a potentially dangling order.
#[derive(Debug, Clone)]
pub struct DanglingOrder {
    /// Symbol of the order
    pub symbol: String,
    /// IBKR order ID
    pub ibkr_id: i32,
    /// Action (buy/sell)
    pub action: String,
    /// Number of shares
    pub shares: i64,
    /// Limit price in cents
    pub limit_price_cents: i64,
    /// Timestamp when the order was submitted
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

/// Verify no dangling orders remain in the audit log.
///
/// This function queries the audit log for `order_submitted` events that do not have
/// a corresponding `order_filled` event. These are considered "dangling" orders that
/// may still be active on the exchange.
///
/// # Arguments
///
/// * `audit_path` - Path to the audit log file
///
/// # Returns
///
/// * `Ok(Vec<DanglingOrder>)` - List of dangling orders (empty if none)
/// * `Err(Error)` - Failed to read or parse the audit log
pub fn verify_no_dangling_orders(audit_path: &Path) -> Result<Vec<DanglingOrder>> {
    info!(
        "Verifying no dangling orders in audit log: {:?}",
        audit_path
    );

    // Read the audit log
    let contents = std::fs::read_to_string(audit_path).map_err(Error::Audit)?;

    // Track submitted orders and filled orders by IBKR ID
    let mut submitted_orders: std::collections::HashMap<i32, DanglingOrder> =
        std::collections::HashMap::new();
    let mut filled_order_ids: std::collections::HashSet<i32> = std::collections::HashSet::new();

    for line in contents.lines() {
        if let Ok(event) = serde_json::from_str::<Value>(line) {
            let event_type = event.get("event").and_then(|e| e.as_str()).unwrap_or("");

            if event_type == "order_submitted" {
                // Extract order details
                let symbol = event
                    .get("symbol")
                    .and_then(|s| s.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let ibkr_id = event.get("ibkr_id").and_then(|i| i.as_i64()).unwrap_or(0) as i32;
                let action = event
                    .get("action")
                    .and_then(|a| a.as_str())
                    .unwrap_or("unknown")
                    .to_string();
                let shares = event.get("shares").and_then(|s| s.as_i64()).unwrap_or(0);
                let limit = event.get("limit").and_then(|l| l.as_f64()).unwrap_or(0.0) as i64;

                // Parse timestamp
                let ts_str = event.get("ts").and_then(|t| t.as_str()).unwrap_or("");
                let timestamp = chrono::DateTime::parse_from_rfc3339(ts_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                let dangling_order = DanglingOrder {
                    symbol,
                    ibkr_id,
                    action,
                    shares,
                    limit_price_cents: limit,
                    timestamp,
                };

                submitted_orders.insert(ibkr_id, dangling_order);
            } else if event_type == "order_filled" {
                let ibkr_id = event.get("ibkr_id").and_then(|i| i.as_i64()).unwrap_or(0) as i32;
                filled_order_ids.insert(ibkr_id);
            }
        }
    }

    // Filter out orders that were filled
    let dangling: Vec<DanglingOrder> = submitted_orders
        .into_iter()
        .filter(|(id, _)| !filled_order_ids.contains(id))
        .map(|(_, order)| order)
        .collect();

    if dangling.is_empty() {
        info!("No dangling orders found");
    } else {
        info!("Found {} potentially dangling orders", dangling.len());
        for order in &dangling {
            info!(
                "  - {} {} {} shares @ ${} (IBKR ID: {})",
                order.action,
                order.symbol,
                order.shares,
                order.limit_price_cents as f64 / 100.0,
                order.ibkr_id
            );
        }
    }

    Ok(dangling)
}

/// Send SIGTERM to a process by PID.
///
/// # Errors
///
/// Returns an error if:
/// - The process does not exist (ESRCH)
/// - Permission is denied to send the signal (EPERM)
/// - Other system errors occur
/// Compile-time kill workflow selected by Cargo features.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KillWorkflow {
    /// Backward-compatible behavior: graceful SIGTERM plus audit-log dangling-order check.
    GracefulOnly,
    /// Guaranteed kill switch behavior: graceful first, then forceful broker cancellation.
    TwoPhase,
}

/// Return the active kill workflow for this build.
pub const fn active_kill_workflow() -> KillWorkflow {
    #[cfg(feature = "guaranteed_kill_switch")]
    {
        KillWorkflow::TwoPhase
    }
    #[cfg(not(feature = "guaranteed_kill_switch"))]
    {
        KillWorkflow::GracefulOnly
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForcefulCancelReport {
    pub attempts: usize,
    pub cancelled_order_ids: Vec<u64>,
    pub remaining_order_ids: Vec<u64>,
    pub errors: Vec<String>,
}

impl ForcefulCancelReport {
    pub fn succeeded(&self) -> bool {
        self.remaining_order_ids.is_empty()
    }
}

fn cancellable_open_orders(orders: Vec<BrokerOrderStatus>) -> Vec<BrokerOrderStatus> {
    orders
        .into_iter()
        .filter(|order| {
            matches!(
                order.status,
                OrderState::Pending | OrderState::Submitted | OrderState::PartiallyFilled
            ) && order.remaining_quantity > 0
        })
        .collect()
}

pub fn forceful_cancel_open_orders_with_retry<F>(
    broker: &dyn Broker,
    max_retries: usize,
    mut sleep_before_retry: F,
) -> ForcefulCancelReport
where
    F: FnMut(usize),
{
    let mut cancelled_order_ids = Vec::new();
    let mut errors = Vec::new();
    let mut remaining_order_ids = Vec::new();

    for attempt in 0..=max_retries {
        let open_orders = match broker.open_orders() {
            Ok(orders) => cancellable_open_orders(orders),
            Err(error) => {
                errors.push(format!(
                    "open_orders attempt {} failed: {error}",
                    attempt + 1
                ));
                if attempt < max_retries {
                    sleep_before_retry(attempt);
                    continue;
                }
                return ForcefulCancelReport {
                    attempts: attempt + 1,
                    cancelled_order_ids,
                    remaining_order_ids,
                    errors,
                };
            }
        };

        if open_orders.is_empty() {
            return ForcefulCancelReport {
                attempts: attempt + 1,
                cancelled_order_ids,
                remaining_order_ids: Vec::new(),
                errors,
            };
        }

        remaining_order_ids = open_orders.iter().map(|order| order.id.0).collect();
        for order in open_orders {
            match broker.cancel_order(order.id) {
                Ok(()) => {
                    if !cancelled_order_ids.contains(&order.id.0) {
                        cancelled_order_ids.push(order.id.0);
                    }
                }
                Err(error) => errors.push(format!("cancel order {} failed: {error}", order.id.0)),
            }
        }

        if attempt < max_retries {
            sleep_before_retry(attempt);
        }
    }

    let remaining_order_ids = match broker.open_orders() {
        Ok(orders) => cancellable_open_orders(orders)
            .into_iter()
            .map(|order| order.id.0)
            .collect(),
        Err(error) => {
            errors.push(format!("final open_orders verification failed: {error}"));
            remaining_order_ids
        }
    };

    ForcefulCancelReport {
        attempts: max_retries + 1,
        cancelled_order_ids,
        remaining_order_ids,
        errors,
    }
}

pub fn forceful_cancel_open_orders(broker: &dyn Broker) -> ForcefulCancelReport {
    forceful_cancel_open_orders_with_retry(broker, 5, |attempt| {
        let backoff_secs = 1_u64 << attempt.min(4);
        thread::sleep(Duration::from_secs(backoff_secs));
    })
}

pub fn kill_timeout_from_sources(config: &Config, cli_timeout_secs: Option<u64>) -> Duration {
    let timeout_secs = cli_timeout_secs
        .filter(|seconds| *seconds > 0)
        .or_else(|| {
            std::env::var("NANOBOOK_KILL_TIMEOUT_SECS")
                .ok()
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|seconds| *seconds > 0)
        })
        .unwrap_or(config.kill.timeout_secs);
    Duration::from_secs(timeout_secs)
}

#[cfg(feature = "guaranteed_kill_switch")]
fn log_kill_phase(audit: &mut AuditLog, event: &'static str, pid: u32) -> Result<()> {
    audit.log(event, serde_json::json!({ "pid": pid }))
}

fn wait_for_process_exit(pid: u32, timeout: Duration) -> Result<bool> {
    #[cfg(unix)]
    {
        use nix::sys::signal;
        use nix::unistd::Pid;

        let nix_pid = Pid::from_raw(pid as i32);
        let start = std::time::Instant::now();

        loop {
            match signal::kill(nix_pid, None) {
                Ok(_) => {
                    if start.elapsed() > timeout {
                        return Ok(false);
                    }
                    thread::sleep(Duration::from_millis(500));
                }
                Err(nix::errno::Errno::ESRCH) => return Ok(true),
                Err(e) => {
                    return Err(Error::Aborted(format!(
                        "Error checking process {}: {}",
                        pid, e
                    )));
                }
            }
        }
    }

    #[cfg(windows)]
    {
        thread::sleep(Duration::from_secs(5));
        Ok(true)
    }
}

pub fn send_sigterm(pid: u32) -> Result<()> {
    info!("Sending SIGTERM to process {}", pid);

    #[cfg(unix)]
    {
        use nix::sys::signal::{self, Signal};
        use nix::unistd::Pid;

        let nix_pid = Pid::from_raw(pid as i32);
        signal::kill(nix_pid, Signal::SIGTERM).map_err(|e| match e {
            nix::errno::Errno::ESRCH => Error::Aborted(format!("Process {} does not exist", pid)),
            nix::errno::Errno::EPERM => Error::Aborted(format!(
                "Permission denied to send signal to process {}",
                pid
            )),
            _ => Error::Aborted(format!("Failed to send SIGTERM to process {}: {}", pid, e)),
        })?;
    }

    #[cfg(windows)]
    {
        // On Windows, we don't have SIGTERM. Use console event instead.
        // This is a simplified implementation - Windows signal handling is more complex.
        use windows::Win32::System::Console::{CTRL_C_EVENT, GenerateConsoleCtrlEvent};

        // Note: This is a simplified approach. On Windows, proper process termination
        // typically requires more complex handling (e.g., using TerminateProcess).
        // For now, we'll return an error indicating this is not fully supported.
        return Err(Error::Aborted(
            "Kill switch not fully supported on Windows yet".to_string(),
        ));
    }

    info!("Successfully sent SIGTERM to process {}", pid);
    Ok(())
}

/// Run the kill switch: send SIGTERM to running runner and verify no dangling orders.
///
/// This function:
/// 1. Reads the PID file to get the running runner's PID
/// 2. Sends SIGTERM to the runner
/// 3. Waits for the process to exit (with timeout)
/// 4. Verifies no dangling orders via audit log
/// 5. Reports results
pub fn run_kill(config: &Config) -> Result<()> {
    run_kill_with_timeout(config, None)
}

#[cfg(feature = "guaranteed_kill_switch")]
pub fn run_kill_with_timeout(config: &Config, cli_timeout_secs: Option<u64>) -> Result<()> {
    run_two_phase_kill(config, kill_timeout_from_sources(config, cli_timeout_secs))
}

#[cfg(not(feature = "guaranteed_kill_switch"))]
pub fn run_kill_with_timeout(config: &Config, cli_timeout_secs: Option<u64>) -> Result<()> {
    run_graceful_kill(config, kill_timeout_from_sources(config, cli_timeout_secs))
}

#[cfg(feature = "guaranteed_kill_switch")]
fn run_two_phase_kill(config: &Config, timeout: Duration) -> Result<()> {
    let pid_path = Path::new(pid_file::DEFAULT_PID_FILE);
    let audit_path = config.audit_path();
    let started = std::time::Instant::now();
    let mut audit = AuditLog::open(&audit_path)?;
    log_kill_requested(&mut audit, "two_phase", "command")?;

    if !pid_file_exists(pid_path) {
        return Err(Error::Aborted(format!(
            "PID file not found at {}. Is a rebalancer running?",
            pid_path.display()
        )));
    }

    let pid = read_pid_file(pid_path)?;
    run_two_phase_kill_for_pid(
        config,
        &audit_path,
        &mut audit,
        pid,
        timeout,
        started,
        send_sigterm,
        wait_for_process_exit,
        |config| {
            let mut broker = IbkrBroker::new(
                &config.connection.host,
                config.connection.port,
                config.connection.client_id,
            );
            broker
                .connect()
                .map_err(|error| Error::Connection(error.to_string()))?;
            Ok(forceful_cancel_open_orders(&broker))
        },
    )
}

#[cfg(feature = "guaranteed_kill_switch")]
fn run_two_phase_kill_for_pid<Send, Wait, Force>(
    config: &Config,
    audit_path: &Path,
    audit: &mut AuditLog,
    pid: u32,
    timeout: Duration,
    started: std::time::Instant,
    send_sigterm_fn: Send,
    wait_for_exit_fn: Wait,
    forceful_cancel_fn: Force,
) -> Result<()>
where
    Send: FnOnce(u32) -> Result<()>,
    Wait: FnOnce(u32, Duration) -> Result<bool>,
    Force: FnOnce(&Config) -> Result<ForcefulCancelReport>,
{
    log_kill_phase(audit, "kill_phase1_started", pid)?;
    send_sigterm_fn(pid)?;

    if wait_for_exit_fn(pid, timeout)? {
        log_kill_phase(audit, "kill_phase1_completed", pid)?;
        let dangling = verify_no_dangling_orders(audit_path)?;
        log_kill_completed_with_summary(
            audit,
            "graceful",
            0,
            dangling.len(),
            started.elapsed().as_secs_f64(),
            &[],
        )?;
        if dangling.is_empty() {
            return Ok(());
        }
    }

    log_kill_phase(audit, "kill_phase2_started", pid)?;
    let report = forceful_cancel_fn(config)?;
    audit.log(
        "kill_phase2_completed",
        serde_json::json!({
            "pid": pid,
            "attempts": report.attempts,
            "orders_cancelled_count": report.cancelled_order_ids.len(),
            "orders_remaining_count": report.remaining_order_ids.len(),
            "cancelled_order_ids": &report.cancelled_order_ids,
            "remaining_order_ids": &report.remaining_order_ids,
            "error_messages": &report.errors,
        }),
    )?;
    log_kill_completed_with_summary(
        audit,
        "forced",
        report.cancelled_order_ids.len(),
        report.remaining_order_ids.len(),
        started.elapsed().as_secs_f64(),
        &report.errors,
    )?;

    if report.succeeded() {
        Ok(())
    } else {
        Err(Error::Aborted(format!(
            "Forceful cancellation incomplete: {} orders remain",
            report.remaining_order_ids.len()
        )))
    }
}

#[cfg(not(feature = "guaranteed_kill_switch"))]
fn run_graceful_kill(config: &Config, timeout: Duration) -> Result<()> {
    let pid_path = Path::new(pid_file::DEFAULT_PID_FILE);
    let audit_path = config.audit_path();

    info!("Starting kill switch procedure");
    let started = std::time::Instant::now();
    let mut audit = AuditLog::open(&audit_path)?;
    log_kill_requested(&mut audit, "graceful", "command")?;

    // Step 1: Check if PID file exists
    if !pid_file_exists(pid_path) {
        return Err(Error::Aborted(format!(
            "PID file not found at {}. Is a rebalancer running?",
            pid_path.display()
        )));
    }

    // Step 2: Read PID from file
    let pid = read_pid_file(pid_path)?;
    info!("Found PID {} in {}", pid, pid_path.display());

    // Step 3: Send SIGTERM to the process
    send_sigterm(pid)?;

    // Step 4: Wait for process to exit (with timeout)
    info!("Waiting for process {} to exit...", pid);
    if !wait_for_process_exit(pid, timeout)? {
        return Err(Error::Aborted(format!(
            "Process {} did not exit within {} seconds timeout",
            pid,
            timeout.as_secs()
        )));
    }
    info!("Process {} has exited", pid);

    // Step 5: Verify no dangling orders
    info!("Verifying no dangling orders in audit log...");
    let dangling = verify_no_dangling_orders(&audit_path)?;

    if !dangling.is_empty() {
        let message = format!(
            "Found {} potentially dangling orders. Manual intervention required.",
            dangling.len()
        );
        log_kill_completed_with_summary(
            &mut audit,
            "graceful",
            0,
            dangling.len(),
            started.elapsed().as_secs_f64(),
            std::slice::from_ref(&message),
        )?;
        return Err(Error::Aborted(message));
    }

    log_kill_completed_with_summary(
        &mut audit,
        "graceful",
        0,
        0,
        started.elapsed().as_secs_f64(),
        &[],
    )?;
    info!("Kill switch completed successfully: process terminated, no dangling orders");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanobook::Symbol;
    use nanobook_broker::error::BrokerError;
    use nanobook_broker::types::{
        Account, BrokerOrder, BrokerOrderStatus, OrderId, OrderState, Position, Quote,
    };
    use std::cell::{Cell, RefCell};
    use std::sync::{Mutex, OnceLock};
    use std::time::SystemTime;
    use tempfile::NamedTempFile;

    struct TestBroker {
        orders: RefCell<Vec<BrokerOrderStatus>>,
        fail_open_attempts: Cell<usize>,
        fail_open_on_call: Cell<Option<usize>>,
        open_calls: Cell<usize>,
        fail_cancel: Cell<bool>,
    }

    impl TestBroker {
        fn with_orders(ids: &[u64]) -> Self {
            Self::with_statuses(
                &ids.iter()
                    .map(|id| (*id, OrderState::Submitted, 1))
                    .collect::<Vec<_>>(),
            )
        }

        fn with_statuses(statuses: &[(u64, OrderState, u64)]) -> Self {
            Self {
                orders: RefCell::new(
                    statuses
                        .iter()
                        .map(|(id, status, remaining_quantity)| BrokerOrderStatus {
                            id: OrderId(*id),
                            status: *status,
                            filled_quantity: 0,
                            remaining_quantity: *remaining_quantity,
                            avg_fill_price_cents: 0,
                        })
                        .collect(),
                ),
                fail_open_attempts: Cell::new(0),
                fail_open_on_call: Cell::new(None),
                open_calls: Cell::new(0),
                fail_cancel: Cell::new(false),
            }
        }
    }

    impl Broker for TestBroker {
        fn connect(&mut self) -> std::result::Result<(), BrokerError> {
            Ok(())
        }

        fn disconnect(&mut self) -> std::result::Result<(), BrokerError> {
            Ok(())
        }

        fn positions(&self) -> std::result::Result<Vec<Position>, BrokerError> {
            Ok(Vec::new())
        }

        fn account(&self) -> std::result::Result<Account, BrokerError> {
            Ok(Account {
                equity_cents: 0,
                buying_power_cents: 0,
                cash_cents: 0,
                gross_position_value_cents: 0,
            })
        }

        fn submit_order(&self, _order: &BrokerOrder) -> std::result::Result<OrderId, BrokerError> {
            Ok(OrderId(1))
        }

        fn order_status(&self, id: OrderId) -> std::result::Result<BrokerOrderStatus, BrokerError> {
            Ok(BrokerOrderStatus {
                id,
                status: OrderState::Submitted,
                filled_quantity: 0,
                remaining_quantity: 1,
                avg_fill_price_cents: 0,
            })
        }

        fn open_orders(&self) -> std::result::Result<Vec<BrokerOrderStatus>, BrokerError> {
            let call = self.open_calls.get() + 1;
            self.open_calls.set(call);
            if self.fail_open_on_call.get() == Some(call) {
                return Err(BrokerError::Connection("open orders unavailable".into()));
            }
            let remaining_failures = self.fail_open_attempts.get();
            if remaining_failures > 0 {
                self.fail_open_attempts.set(remaining_failures - 1);
                return Err(BrokerError::Connection("open orders unavailable".into()));
            }
            Ok(self.orders.borrow().clone())
        }

        fn cancel_order(&self, id: OrderId) -> std::result::Result<(), BrokerError> {
            if self.fail_cancel.get() {
                return Err(BrokerError::Order("cancel failed".into()));
            }
            self.orders.borrow_mut().retain(|order| order.id != id);
            Ok(())
        }

        fn quote(&self, symbol: &Symbol) -> std::result::Result<Quote, BrokerError> {
            Ok(Quote {
                symbol: *symbol,
                bid_cents: 0,
                ask_cents: 0,
                last_cents: 0,
                volume: 0,
                timestamp: SystemTime::now(),
            })
        }
    }

    #[test]
    #[cfg(not(feature = "guaranteed_kill_switch"))]
    fn test_default_kill_workflow_is_graceful_only() {
        assert_eq!(active_kill_workflow(), KillWorkflow::GracefulOnly);
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn test_guaranteed_kill_switch_selects_two_phase_workflow() {
        assert_eq!(active_kill_workflow(), KillWorkflow::TwoPhase);
    }

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn test_config_with_kill_timeout(timeout_secs: u64) -> Config {
        let toml = format!(
            r#"
[connection]
host = "127.0.0.1"
port = 4002
client_id = 100

[account]
id = "DU123456"
type = "margin"

[execution]
order_interval_ms = 100
limit_offset_bps = 5
order_timeout_secs = 300
max_orders_per_run = 50
quote_staleness_threshold_sec = 30

[risk]
max_position_pct = 0.25
max_leverage = 1.5
min_trade_usd = 100.0
max_trade_usd = 100000.0
allow_short = true
max_short_pct = 0.30

[cost]
commission_per_share = 0.0035
commission_min = 0.35
slippage_bps = 5

[logging]
dir = "./logs"
audit_file = "audit.jsonl"
clock_skew_threshold_sec = 30
max_jump_rate_sec_per_sec = 2.0

[kill]
timeout_secs = {timeout_secs}
"#
        );
        toml::from_str(&toml).expect("test config should parse")
    }

    #[test]
    fn timeout_defaults_to_config() {
        let _guard = match env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        unsafe {
            std::env::remove_var("NANOBOOK_KILL_TIMEOUT_SECS");
        }
        let config = test_config_with_kill_timeout(17);

        assert_eq!(
            kill_timeout_from_sources(&config, None),
            Duration::from_secs(17)
        );
    }

    #[test]
    fn timeout_uses_environment_override() {
        let _guard = match env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let config = test_config_with_kill_timeout(17);
        unsafe {
            std::env::set_var("NANOBOOK_KILL_TIMEOUT_SECS", "19");
        }
        let timeout = kill_timeout_from_sources(&config, None);
        unsafe {
            std::env::remove_var("NANOBOOK_KILL_TIMEOUT_SECS");
        }

        assert_eq!(timeout, Duration::from_secs(19));
    }

    #[test]
    fn timeout_uses_cli_override_before_environment() {
        let _guard = match env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let config = test_config_with_kill_timeout(17);
        unsafe {
            std::env::set_var("NANOBOOK_KILL_TIMEOUT_SECS", "19");
        }
        let timeout = kill_timeout_from_sources(&config, Some(23));
        unsafe {
            std::env::remove_var("NANOBOOK_KILL_TIMEOUT_SECS");
        }

        assert_eq!(timeout, Duration::from_secs(23));
    }

    #[test]
    fn verify_no_dangling_orders_errors_for_missing_audit_log() {
        let path = std::path::PathBuf::from("/tmp/nanobook-missing-audit-log-for-test.jsonl");
        let _ = std::fs::remove_file(&path);

        assert!(verify_no_dangling_orders(&path).is_err());
    }

    #[test]
    fn verify_no_dangling_orders_handles_malformed_timestamp_and_logs_dangling() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(
            temp_file.path(),
            r#"{"event":"order_submitted","ts":"not-a-time","symbol":"AAPL","action":"Buy","shares":1,"limit":12345,"ibkr_id":77}
"#,
        )
        .unwrap();

        let dangling = verify_no_dangling_orders(temp_file.path()).unwrap();

        assert_eq!(dangling.len(), 1);
        assert_eq!(dangling[0].symbol, "AAPL");
        assert_eq!(dangling[0].ibkr_id, 77);
    }

    #[test]
    fn wait_for_missing_process_reports_exited() {
        assert!(wait_for_process_exit(999_999, Duration::from_millis(1)).unwrap());
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn run_kill_reports_nonexistent_pid_from_pid_file() {
        let _guard = match env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let original_dir = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_kill_timeout(30);
        config.logging.dir = dir.path().join("logs").display().to_string();
        std::env::set_current_dir(dir.path()).unwrap();
        std::fs::write(pid_file::DEFAULT_PID_FILE, "999999").unwrap();

        let result = run_kill(&config);

        std::env::set_current_dir(original_dir).unwrap();
        assert!(
            matches!(result, Err(Error::Aborted(message)) if message.contains("does not exist"))
        );
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn run_kill_reports_missing_pid_file() {
        let _guard = match env_lock().lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        let original_dir = std::env::current_dir().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut config = test_config_with_kill_timeout(30);
        config.logging.dir = dir.path().join("logs").display().to_string();
        std::env::set_current_dir(dir.path()).unwrap();

        let result = run_kill(&config);

        std::env::set_current_dir(original_dir).unwrap();
        assert!(
            matches!(result, Err(Error::Aborted(message)) if message.contains("PID file not found"))
        );
    }

    #[test]
    fn test_broker_stub_methods_return_defaults() {
        let mut broker = TestBroker::with_orders(&[7]);
        broker.connect().unwrap();
        broker.disconnect().unwrap();
        assert_eq!(broker.positions().unwrap().len(), 0);
        assert_eq!(broker.account().unwrap().cash_cents, 0);
        let order = BrokerOrder {
            symbol: Symbol::new("AAPL"),
            side: nanobook_broker::types::BrokerSide::Buy,
            quantity: 1,
            order_type: nanobook_broker::types::BrokerOrderType::Market,
            client_order_id: None,
        };
        assert_eq!(broker.submit_order(&order).unwrap(), OrderId(1));
        assert_eq!(broker.order_status(OrderId(7)).unwrap().id, OrderId(7));
        assert_eq!(
            broker.quote(&Symbol::new("AAPL")).unwrap().symbol,
            Symbol::new("AAPL")
        );
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn two_phase_skips_forceful_when_phase1_succeeds() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let config = test_config_with_kill_timeout(30);
        {
            let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
            run_two_phase_kill_for_pid(
                &config,
                &audit_path,
                &mut audit,
                4242,
                Duration::from_secs(30),
                std::time::Instant::now(),
                |_| Ok(()),
                |_, _| Ok(true),
                |_| panic!("phase 2 should not run when phase 1 succeeds"),
            )
            .unwrap();
        }

        let events = crate::audit::parse_audit_events(&audit_path).unwrap();
        let names: Vec<_> = events.iter().map(|event| event.event.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "kill_phase1_started",
                "kill_phase1_completed",
                "kill_completed"
            ]
        );
        assert_eq!(events[2].data["method"], "graceful");
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn two_phase_runs_forceful_when_phase1_times_out() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let config = test_config_with_kill_timeout(30);
        {
            let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
            run_two_phase_kill_for_pid(
                &config,
                &audit_path,
                &mut audit,
                4242,
                Duration::from_secs(30),
                std::time::Instant::now(),
                |_| Ok(()),
                |_, _| Ok(false),
                |_| {
                    Ok(ForcefulCancelReport {
                        attempts: 2,
                        cancelled_order_ids: vec![10, 11],
                        remaining_order_ids: vec![],
                        errors: vec![],
                    })
                },
            )
            .unwrap();
        }

        let events = crate::audit::parse_audit_events(&audit_path).unwrap();
        let names: Vec<_> = events.iter().map(|event| event.event.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "kill_phase1_started",
                "kill_phase2_started",
                "kill_phase2_completed",
                "kill_completed"
            ]
        );
        assert_eq!(events[2].data["orders_cancelled_count"], 2);
        assert_eq!(events[3].data["method"], "forced");
    }

    #[test]
    #[cfg(feature = "guaranteed_kill_switch")]
    fn two_phase_returns_error_when_forceful_leaves_orders() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let config = test_config_with_kill_timeout(30);
        {
            let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
            let result = run_two_phase_kill_for_pid(
                &config,
                &audit_path,
                &mut audit,
                4242,
                Duration::from_secs(30),
                std::time::Instant::now(),
                |_| Ok(()),
                |_, _| Ok(false),
                |_| {
                    Ok(ForcefulCancelReport {
                        attempts: 6,
                        cancelled_order_ids: vec![10],
                        remaining_order_ids: vec![11],
                        errors: vec!["cancel_order(11) failed: cancel failed".into()],
                    })
                },
            );
            assert!(result.is_err());
        }

        let events = crate::audit::parse_audit_events(&audit_path).unwrap();
        assert_eq!(events[2].data["orders_remaining_count"], 1);
        assert_eq!(events[2].data["remaining_order_ids"][0], 11);
        assert_eq!(events[3].data["orders_remaining_count"], 1);
    }

    #[test]
    fn cancellable_open_orders_filters_terminal_and_zero_remaining_orders() {
        let orders = vec![
            BrokerOrderStatus {
                id: OrderId(1),
                status: OrderState::Pending,
                filled_quantity: 0,
                remaining_quantity: 1,
                avg_fill_price_cents: 0,
            },
            BrokerOrderStatus {
                id: OrderId(2),
                status: OrderState::Submitted,
                filled_quantity: 0,
                remaining_quantity: 1,
                avg_fill_price_cents: 0,
            },
            BrokerOrderStatus {
                id: OrderId(3),
                status: OrderState::PartiallyFilled,
                filled_quantity: 1,
                remaining_quantity: 1,
                avg_fill_price_cents: 0,
            },
            BrokerOrderStatus {
                id: OrderId(4),
                status: OrderState::Filled,
                filled_quantity: 1,
                remaining_quantity: 0,
                avg_fill_price_cents: 0,
            },
            BrokerOrderStatus {
                id: OrderId(5),
                status: OrderState::Cancelled,
                filled_quantity: 0,
                remaining_quantity: 0,
                avg_fill_price_cents: 0,
            },
            BrokerOrderStatus {
                id: OrderId(6),
                status: OrderState::Submitted,
                filled_quantity: 1,
                remaining_quantity: 0,
                avg_fill_price_cents: 0,
            },
        ];

        let ids: Vec<_> = cancellable_open_orders(orders)
            .into_iter()
            .map(|order| order.id.0)
            .collect();

        assert_eq!(ids, vec![1, 2, 3]);
    }

    #[test]
    fn forceful_cancel_ignores_non_cancellable_open_orders() {
        let broker = TestBroker::with_statuses(&[
            (1, OrderState::Filled, 0),
            (2, OrderState::Cancelled, 0),
            (3, OrderState::Submitted, 0),
        ]);
        let report = forceful_cancel_open_orders_with_retry(&broker, 1, |_| {});

        assert!(report.succeeded());
        assert!(report.cancelled_order_ids.is_empty());
        assert_eq!(report.attempts, 1);
    }

    #[test]
    fn forceful_cancel_reports_query_failure_after_max_retries() {
        let broker = TestBroker::with_orders(&[10]);
        broker.fail_open_attempts.set(3);
        let mut slept = Vec::new();
        let report = forceful_cancel_open_orders_with_retry(&broker, 2, |attempt| {
            slept.push(attempt);
        });

        assert_eq!(report.attempts, 3);
        assert_eq!(slept, vec![0, 1]);
        assert_eq!(report.errors.len(), 3);
        assert!(report.cancelled_order_ids.is_empty());
    }

    #[test]
    fn forceful_cancel_keeps_previous_remaining_when_final_verification_fails() {
        let broker = TestBroker::with_orders(&[10]);
        broker.fail_open_on_call.set(Some(2));
        broker.fail_cancel.set(true);
        let report = forceful_cancel_open_orders_with_retry(&broker, 0, |_| {});

        assert!(!report.succeeded());
        assert_eq!(report.remaining_order_ids, vec![10]);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("final open_orders verification failed"))
        );
    }

    #[test]
    fn forceful_cancel_default_wrapper_uses_retry_policy() {
        let broker = TestBroker::with_orders(&[]);
        broker.fail_open_attempts.set(1);
        let report = forceful_cancel_open_orders(&broker);

        assert!(report.succeeded());
        assert_eq!(report.attempts, 2);
    }

    #[test]
    fn forceful_cancel_cancels_all_open_orders() {
        let broker = TestBroker::with_orders(&[10, 11]);
        let report = forceful_cancel_open_orders_with_retry(&broker, 1, |_| {});

        assert!(report.succeeded());
        assert_eq!(report.cancelled_order_ids, vec![10, 11]);
        assert!(broker.open_orders().unwrap_or_default().is_empty());
    }

    #[test]
    fn forceful_cancel_retries_open_order_query_failure() {
        let broker = TestBroker::with_orders(&[42]);
        broker.fail_open_attempts.set(1);
        let mut sleeps = Vec::new();
        let report = forceful_cancel_open_orders_with_retry(&broker, 2, |attempt| {
            sleeps.push(attempt);
        });

        assert!(report.succeeded());
        assert_eq!(report.attempts, 3);
        assert_eq!(sleeps, vec![0, 1]);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("open_orders"))
        );
    }

    #[test]
    fn forceful_cancel_reports_remaining_orders_after_cancel_failure() {
        let broker = TestBroker::with_orders(&[99]);
        broker.fail_cancel.set(true);
        let report = forceful_cancel_open_orders_with_retry(&broker, 1, |_| {});

        assert!(!report.succeeded());
        assert_eq!(report.remaining_order_ids, vec![99]);
        assert!(
            report
                .errors
                .iter()
                .any(|error| error.contains("cancel order 99"))
        );
    }

    #[test]
    #[cfg(unix)]
    fn test_send_sigterm_to_child_process() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .unwrap();
        send_sigterm(child.id()).unwrap();
        let _ = child.wait().unwrap();
    }

    #[test]
    #[cfg(unix)]
    fn test_send_sigterm_to_nonexistent_process() {
        // Try to send SIGTERM to a non-existent process
        let result = send_sigterm(999999);
        assert!(result.is_err());
    }

    #[test]
    fn test_verify_no_dangling_orders_empty_log() {
        let temp_file = NamedTempFile::new().unwrap();
        std::fs::write(temp_file.path(), "").unwrap();

        let result = verify_no_dangling_orders(temp_file.path());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_verify_no_dangling_orders_with_filled_orders() {
        let temp_file = NamedTempFile::new().unwrap();
        let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_filled","ts":"2024-01-01T00:00:01Z","symbol":"AAPL","ibkr_id":1,"filled":100,"avg_price":150.00}"#;
        std::fs::write(temp_file.path(), audit_log).unwrap();

        let result = verify_no_dangling_orders(temp_file.path());
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_verify_no_dangling_orders_with_dangling_orders() {
        let temp_file = NamedTempFile::new().unwrap();
        let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_submitted","ts":"2024-01-01T00:00:01Z","symbol":"MSFT","action":"Sell","shares":50,"limit":400.00,"ibkr_id":2}"#;
        std::fs::write(temp_file.path(), audit_log).unwrap();

        let result = verify_no_dangling_orders(temp_file.path());
        assert!(result.is_ok());
        let dangling = result.unwrap();
        assert_eq!(dangling.len(), 2);
        // Don't depend on HashMap order - just check that both symbols are present
        let symbols: Vec<&str> = dangling.iter().map(|o| o.symbol.as_str()).collect();
        assert!(symbols.contains(&"AAPL"));
        assert!(symbols.contains(&"MSFT"));
    }
}
