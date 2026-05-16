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
use log::info;
use serde_json::Value;
use std::path::Path;
use std::thread;
use std::time::Duration;

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
#[cfg(feature = "guaranteed_kill_switch")]
pub fn run_kill(config: &Config) -> Result<()> {
    run_two_phase_kill(config)
}

#[cfg(not(feature = "guaranteed_kill_switch"))]
pub fn run_kill(config: &Config) -> Result<()> {
    run_graceful_kill(config)
}

#[cfg(feature = "guaranteed_kill_switch")]
fn run_two_phase_kill(config: &Config) -> Result<()> {
    // Phase-2 broker-side cancellation is implemented in the follow-up forceful
    // cancellation bead. Until then, this feature-gated entry point delegates to
    // the proven graceful path so enabling the flag cannot regress production
    // behavior or bypass dangling-order verification.
    run_graceful_kill(config)
}

fn run_graceful_kill(config: &Config) -> Result<()> {
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
    let timeout = Duration::from_secs(30);
    let start = std::time::Instant::now();

    #[cfg(unix)]
    {
        use nix::sys::signal;
        use nix::unistd::Pid;

        let nix_pid = Pid::from_raw(pid as i32);

        loop {
            // Check if process still exists by sending signal 0 (no signal)
            match signal::kill(nix_pid, None) {
                Ok(_) => {
                    // Process still exists
                    if start.elapsed() > timeout {
                        return Err(Error::Aborted(format!(
                            "Process {} did not exit within {} seconds timeout",
                            pid,
                            timeout.as_secs()
                        )));
                    }
                    thread::sleep(Duration::from_millis(500));
                }
                Err(nix::errno::Errno::ESRCH) => {
                    // Process does not exist - it has exited
                    info!("Process {} has exited", pid);
                    break;
                }
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
        // On Windows, we can't easily wait for process exit without more complex handling
        // For now, just wait a fixed time and assume the process exits
        thread::sleep(Duration::from_secs(5));
        info!("Assumed process {} has exited (Windows)", pid);
    }

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
    use tempfile::NamedTempFile;

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

    #[test]
    #[cfg(unix)]
    fn test_send_sigterm_to_self() {
        // Sending SIGTERM to ourselves should succeed
        let _pid = std::process::id();
        // Note: This will actually terminate the test if not handled,
        // so we skip this test for now
        // send_sigterm(pid).unwrap();
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
