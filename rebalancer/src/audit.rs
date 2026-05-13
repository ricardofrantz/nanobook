//! JSONL audit trail logging.
//!
//! Each rebalancer run appends events to an audit.jsonl file,
//! one JSON object per line (following nanobook's persistence pattern).
//!
//! # Clock Skew Detection
//!
//! The audit log includes clock skew detection to identify anomalous
//! timestamp jumps caused by NTP drift, VM clock adjustments, or other
//! system clock issues. When skew is detected, a WARN-level log message
//! is emitted, but logging continues — the audit log does not block
//! operations due to clock issues. This ensures audit trail integrity
//! while alerting operators to investigate potential clock problems.

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use log::warn;

use crate::clock_skew::{ClockSkewDetector, SkewResult};
use crate::error::{Error, Result};

/// Checkpoint events for crash recovery.
///
/// These are the key events that mark progress through a rebalance run.
/// The recovery system uses these checkpoints to determine where to
/// resume after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Checkpoint {
    /// Rebalance run begins
    RunStarted,
    /// Current positions retrieved from broker
    PositionsFetched,
    /// Rebalance diff computed
    DiffComputed,
    /// Risk checks passed
    RiskCheckPassed,
    /// Individual order submitted (symbol is encoded in event data)
    OrderSubmitted,
    /// Individual order filled (symbol is encoded in event data)
    OrderFilled,
    /// Rebalance run completes (success or failure)
    RunCompleted,
}

impl Checkpoint {
    /// Convert checkpoint to event name string
    pub fn as_event_name(&self) -> &'static str {
        match self {
            Checkpoint::RunStarted => "run_started",
            Checkpoint::PositionsFetched => "positions_fetched",
            Checkpoint::DiffComputed => "diff_computed",
            Checkpoint::RiskCheckPassed => "risk_check_passed",
            Checkpoint::OrderSubmitted => "order_submitted",
            Checkpoint::OrderFilled => "order_filled",
            Checkpoint::RunCompleted => "run_completed",
        }
    }

    /// Parse event name string to checkpoint
    pub fn from_event_name(name: &str) -> Option<Self> {
        match name {
            "run_started" => Some(Checkpoint::RunStarted),
            "positions_fetched" => Some(Checkpoint::PositionsFetched),
            "diff_computed" => Some(Checkpoint::DiffComputed),
            "risk_check_passed" => Some(Checkpoint::RiskCheckPassed),
            "order_submitted" => Some(Checkpoint::OrderSubmitted),
            "order_filled" => Some(Checkpoint::OrderFilled),
            "run_completed" => Some(Checkpoint::RunCompleted),
            _ => None,
        }
    }
}

/// An audit event written to the JSONL trail.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditEvent {
    pub event: String,
    pub ts: DateTime<Utc>,
    /// Sequence number for cron mode idempotency and crash recovery (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sequence_number: Option<u64>,
    /// Window ID derived from target spec (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window_id: Option<String>,
    /// Checkpoint type for crash recovery (optional)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkpoint: Option<String>,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Append-only audit logger.
///
/// # File permissions
///
/// On Unix, newly-created audit files are owner-read/write only
/// (`mode 0o600`). The file records positions, equity, and order
/// details — defending it from accidental world-readability via a
/// lax umask is cheap and protects against leaks through shared
/// filesystems, misconfigured backups, or inadvertent commits.
///
/// On Windows, permissions are inherited from the parent directory
/// and are NOT restricted by nanobook. Users on shared Windows
/// systems should set ACLs on the audit directory manually.
///
/// The mode is applied only on file *creation*. Pre-existing audit
/// files keep their current permissions — if you need to tighten an
/// existing file, remove it and let nanobook recreate it, or call
/// `chmod 600 <path>` out of band.
///
/// # Clock skew detection
///
/// The audit log includes a clock skew detector that checks for
/// anomalous timestamp jumps caused by NTP drift, VM clock adjustments,
/// or other system clock issues. When skew is detected, a WARN-level
/// log message is emitted, but logging continues — the audit log
/// does not block operations due to clock issues. This ensures audit
/// trail integrity while alerting operators to investigate potential
/// clock problems.
#[derive(Debug)]
pub struct AuditLog {
    path: PathBuf,
    writer: BufWriter<std::fs::File>,
    clock_skew_detector: ClockSkewDetector,
}

impl AuditLog {
    /// Open (or create) the audit log file for appending.
    ///
    /// Validates that `path` canonicalizes to a location under the
    /// current working directory — symlink-aware, so a symlink
    /// escape like `./logs → /tmp/shared` is rejected. On Unix, a
    /// freshly-created file receives mode `0o600`. See the
    /// type-level doc for the Windows caveat.
    ///
    /// # Errors
    ///
    /// - [`Error::AuditPathOutsideWorkdir`] if `path` resolves
    ///   outside CWD (including through a symlink).
    /// - `Error::Audit(io::Error)` for filesystem-level failures
    ///   (CWD unreadable, permission denied on create, etc.).
    pub fn open(path: &Path) -> Result<Self> {
        let workdir = std::env::current_dir()?;
        Self::open_in(path, &workdir)
    }

    /// Open (or create) the audit log file for appending with a custom
    /// clock skew detector.
    ///
    /// This variant is primarily useful for testing, allowing injection
    /// of a detector with custom thresholds or pre-configured state.
    pub fn open_with_detector(path: &Path, detector: ClockSkewDetector) -> Result<Self> {
        let workdir = std::env::current_dir()?;
        Self::open_in_with_detector(path, &workdir, detector)
    }

    /// Open (or create) the audit log file for appending, validating
    /// that `path` resolves to a location under `workdir`.
    ///
    /// Use this variant when the "allowed root" is not the process
    /// CWD — typical cases are tests working in a `tempdir` and a
    /// future `--workdir` CLI flag. See [`Self::open`] for the
    /// ergonomic default.
    pub fn open_in(path: &Path, workdir: &Path) -> Result<Self> {
        Self::open_in_with_detector(path, workdir, ClockSkewDetector::new())
    }

    /// Open (or create) the audit log file for appending with a custom
    /// clock skew detector, validating that `path` resolves to a
    /// location under `workdir`.
    pub fn open_in_with_detector(
        path: &Path,
        workdir: &Path,
        detector: ClockSkewDetector,
    ) -> Result<Self> {
        validate_audit_path(path, workdir)?;

        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let mut opts = OpenOptions::new();
        opts.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let file = opts.open(path)?;

        Ok(Self {
            path: path.to_path_buf(),
            writer: BufWriter::new(file),
            clock_skew_detector: detector,
        })
    }

    /// Log an event with arbitrary JSON data.
    pub fn log(&mut self, event: &'static str, data: serde_json::Value) -> Result<()> {
        self.log_with_idempotency(event, None, None, data)
    }

    /// Log an event with optional idempotency fields (sequence_number, window_id).
    pub fn log_with_idempotency(
        &mut self,
        event: &'static str,
        sequence_number: Option<u64>,
        window_id: Option<String>,
        data: serde_json::Value,
    ) -> Result<()> {
        let ts = Utc::now();

        // Check for clock skew before logging
        match self.clock_skew_detector.check(ts) {
            SkewResult::Ok => {}
            SkewResult::BackwardJump { duration } => {
                warn!(
                    "Clock skew detected: backward jump of {} seconds",
                    duration.num_seconds()
                );
            }
            SkewResult::ForwardJump { duration, rate } => {
                warn!(
                    "Clock skew detected: forward jump of {} seconds (rate: {:.2}x)",
                    duration.num_seconds(),
                    rate
                );
            }
        }

        let entry = AuditEvent {
            event: event.to_string(),
            ts,
            sequence_number,
            window_id,
            checkpoint: None,
            data,
        };
        let json = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{json}")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Log a checkpoint event for crash recovery.
    ///
    /// Checkpoints are critical events that mark progress through a rebalance run.
    /// They include a sequence number and are fsynced to ensure durability.
    /// The recovery system uses these checkpoints to determine where to resume after a crash.
    pub fn log_checkpoint(
        &mut self,
        checkpoint: Checkpoint,
        sequence_number: u64,
        data: serde_json::Value,
    ) -> Result<()> {
        let ts = Utc::now();

        // Check for clock skew before logging
        match self.clock_skew_detector.check(ts) {
            SkewResult::Ok => {}
            SkewResult::BackwardJump { duration } => {
                warn!(
                    "Clock skew detected: backward jump of {} seconds",
                    duration.num_seconds()
                );
            }
            SkewResult::ForwardJump { duration, rate } => {
                warn!(
                    "Clock skew detected: forward jump of {} seconds (rate: {:.2}x)",
                    duration.num_seconds(),
                    rate
                );
            }
        }

        let entry = AuditEvent {
            event: checkpoint.as_event_name().to_string(),
            ts,
            sequence_number: Some(sequence_number),
            window_id: None,
            checkpoint: Some(format!("{:?}", checkpoint)),
            data,
        };
        let json = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{json}")?;
        self.writer.flush()?;

        // Fsync to ensure checkpoint is durable
        self.writer.get_ref().sync_all()?;

        Ok(())
    }

    /// Log a simple event with no additional data.
    pub fn log_simple(&mut self, event: &'static str) -> Result<()> {
        self.log(event, serde_json::json!({}))
    }

    /// Check if a rebalance window was already completed in cron mode.
    ///
    /// This method reads the audit log and searches for `cron_completed` events
    /// with a matching `window_id`. If found, it returns the sequence number
    /// of the most recent completion. This is used to prevent double-firing
    /// the same rebalance window in cron mode.
    ///
    /// # Arguments
    ///
    /// * `window_id` - The window identifier derived from the target specification
    ///
    /// # Returns
    ///
    /// * `Ok(Some(sequence_number))` - The window is complete with this sequence number
    /// * `Ok(None)` - The window has not been completed yet
    /// * `Err(Error)` - Failed to read or parse the audit log
    ///
    /// # Idempotency Behavior
    ///
    /// Only `cron_completed` events are considered. Other events like `cron_start`
    /// or `run_started` do not mark a window as complete. This ensures that
    /// incomplete or failed runs do not block subsequent attempts.
    pub fn check_window_already_complete(
        &mut self,
        window_id: &str,
    ) -> Result<Option<u64>> {
        // Flush any buffered writes first
        self.writer.flush()?;

        // Read the audit log file
        let contents = std::fs::read_to_string(&self.path)?;

        // Parse each line and look for cron_completed events with matching window_id
        // Return the most recent (last) completion
        let mut last_sequence: Option<u64> = None;
        for line in contents.lines() {
            if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
                if event.event == "cron_completed" {
                    if event.window_id.as_deref() == Some(window_id) {
                        // Found a completed event for this window
                        if let Some(seq) = event.sequence_number {
                            last_sequence = Some(seq);
                        }
                    }
                }
            }
        }

        Ok(last_sequence)
    }

    /// Validate checkpoint sequence in the audit log.
    ///
    /// This method reads the audit log and validates that:
    /// 1. Checkpoint sequence numbers are monotonic (always increasing)
    /// 2. No checkpoints are missing between the first and last checkpoint
    /// 3. Checkpoint data is not corrupted (valid JSON, required fields present)
    ///
    /// # Returns
    ///
    /// * `Ok(())` - All checkpoints are valid
    /// * `Err(Error)` - Checkpoint validation failed with details
    pub fn validate_checkpoints(&mut self) -> Result<()> {
        // Flush any buffered writes first
        self.writer.flush()?;

        // Read the audit log file
        let contents = std::fs::read_to_string(&self.path)?;

        // Parse checkpoint events and validate sequence
        let mut last_sequence: Option<u64> = None;
        let expected_checkpoints: Vec<Checkpoint> = vec![
            Checkpoint::RunStarted,
            Checkpoint::PositionsFetched,
            Checkpoint::DiffComputed,
            Checkpoint::RiskCheckPassed,
        ];
        let mut found_checkpoints: Vec<Checkpoint> = Vec::new();

        for (line_num, line) in contents.lines().enumerate() {
            if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
                // Check if this is a checkpoint event
                if event.checkpoint.is_some() {
                    if let Some(checkpoint) = Checkpoint::from_event_name(&event.event) {
                        // Validate sequence number is present
                        let sequence_number = event.sequence_number.ok_or_else(|| {
                            Error::AuditValidation(format!(
                                "Checkpoint at line {} missing sequence number",
                                line_num + 1
                            ))
                        })?;

                        // Validate monotonic sequence numbers
                        if let Some(last_seq) = last_sequence {
                            if sequence_number <= last_seq {
                                return Err(Error::AuditValidation(format!(
                                    "Checkpoint sequence not monotonic at line {}: got {}, expected > {}",
                                    line_num + 1, sequence_number, last_seq
                                )));
                            }
                        }
                        last_sequence = Some(sequence_number);

                        // Track found checkpoints
                        found_checkpoints.push(checkpoint);
                    }
                }
            } else if line.trim().is_empty() {
                // Skip empty lines
                continue;
            } else {
                return Err(Error::AuditValidation(format!(
                    "Corrupted audit log at line {}: invalid JSON",
                    line_num + 1
                )));
            }
        }

        // Validate that expected checkpoints are present (in order)
        // Note: OrderSubmitted and OrderFilled can appear multiple times,
        // so we only validate the core sequence up to RiskCheckPassed
        for (i, expected) in expected_checkpoints.iter().enumerate() {
            if found_checkpoints.get(i) != Some(expected) {
                return Err(Error::AuditValidation(format!(
                    "Missing checkpoint: expected {:?} at position {}, found {:?}",
                    expected,
                    i,
                    found_checkpoints.get(i)
                )));
            }
        }

        Ok(())
    }
}

/// Walk `path` upward until finding an existing ancestor that
/// `canonicalize`s, then rejoin the unresolved suffix. This lets
/// callers pass an audit path whose target directory doesn't exist
/// yet (common on first run), while still resolving any symlinks
/// present in the existing prefix.
fn canonicalize_as_far_as_possible(path: &Path) -> std::io::Result<PathBuf> {
    // Fast path: whole thing exists.
    if let Ok(p) = path.canonicalize() {
        return Ok(p);
    }

    // Walk ancestors. For each step up, accumulate the stripped tail
    // so we can rejoin it to the canonical prefix once we find one.
    let mut tail: Vec<&std::ffi::OsStr> = Vec::new();
    let mut cursor = path;
    loop {
        match cursor.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => {
                // Store the component we're stripping.
                if let Some(name) = cursor.file_name() {
                    tail.push(name);
                }
                if let Ok(canon) = parent.canonicalize() {
                    let mut out = canon;
                    for segment in tail.iter().rev() {
                        out.push(segment);
                    }
                    return Ok(out);
                }
                cursor = parent;
            }
            // Reached the root (or an empty parent) without finding
            // anything canonicalizable. Return the original path's
            // canonicalize error so the caller sees a familiar
            // `NotFound`.
            _ => return path.canonicalize(),
        }
    }
}

/// Ensure `path` resolves to a filesystem location under `workdir`.
/// `canonicalize` is symlink-aware, so the check also rejects
/// symlinks in the existing portion of `path` that point outside
/// `workdir`.
fn validate_audit_path(path: &Path, workdir: &Path) -> Result<()> {
    let canonical_path = canonicalize_as_far_as_possible(path)?;
    let canonical_workdir = workdir.canonicalize()?;
    if !canonical_path.starts_with(&canonical_workdir) {
        return Err(Error::AuditPathOutsideWorkdir {
            path: canonical_path,
        });
    }
    Ok(())
}

/// Convenience: log a run start event.
pub fn log_run_started(audit: &mut AuditLog, target_file: &str, account_id: &str) -> Result<()> {
    audit.log(
        "run_started",
        serde_json::json!({
            "target_file": target_file,
            "account": account_id,
        }),
    )
}

/// Convenience: log run started as a checkpoint.
pub fn log_run_started_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    target_file: &str,
    account_id: &str,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::RunStarted,
        sequence_number,
        serde_json::json!({
            "target_file": target_file,
            "account": account_id,
        }),
    )
}

/// Convenience: log positions fetched.
pub fn log_positions(
    audit: &mut AuditLog,
    positions: &[crate::diff::CurrentPosition],
    equity_cents: i64,
) -> Result<()> {
    let pos_data: Vec<_> = positions
        .iter()
        .map(|p| {
            serde_json::json!({
                "symbol": p.symbol.as_str(),
                "qty": p.quantity,
                "avg_cost": p.avg_cost_cents as f64 / 100.0,
            })
        })
        .collect();

    audit.log(
        "positions_fetched",
        serde_json::json!({
            "positions": pos_data,
            "equity": equity_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log positions fetched as a checkpoint.
pub fn log_positions_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    positions: &[crate::diff::CurrentPosition],
    equity_cents: i64,
) -> Result<()> {
    let pos_data: Vec<_> = positions
        .iter()
        .map(|p| {
            serde_json::json!({
                "symbol": p.symbol.as_str(),
                "qty": p.quantity,
                "avg_cost": p.avg_cost_cents as f64 / 100.0,
            })
        })
        .collect();

    audit.log_checkpoint(
        Checkpoint::PositionsFetched,
        sequence_number,
        serde_json::json!({
            "positions": pos_data,
            "equity": equity_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log computed diff.
pub fn log_diff(audit: &mut AuditLog, orders: &[crate::diff::RebalanceOrder]) -> Result<()> {
    let order_data: Vec<_> = orders
        .iter()
        .map(|o| {
            serde_json::json!({
                "symbol": o.symbol.as_str(),
                "action": format!("{}", o.action),
                "shares": o.shares,
                "limit": o.limit_price_cents as f64 / 100.0,
                "description": o.description,
            })
        })
        .collect();

    audit.log("diff_computed", serde_json::json!({ "orders": order_data }))
}

/// Convenience: log computed diff as a checkpoint.
pub fn log_diff_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    orders: &[crate::diff::RebalanceOrder],
) -> Result<()> {
    let order_data: Vec<_> = orders
        .iter()
        .map(|o| {
            serde_json::json!({
                "symbol": o.symbol.as_str(),
                "action": format!("{}", o.action),
                "shares": o.shares,
                "limit": o.limit_price_cents as f64 / 100.0,
                "description": o.description,
            })
        })
        .collect();

    audit.log_checkpoint(
        Checkpoint::DiffComputed,
        sequence_number,
        serde_json::json!({ "orders": order_data }),
    )
}

/// Convenience: log risk check results.
pub fn log_risk_check(audit: &mut AuditLog, report: &crate::risk::RiskReport) -> Result<()> {
    let check_data: Vec<_> = report
        .checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "status": format!("{}", c.status),
                "detail": c.detail,
            })
        })
        .collect();

    audit.log(
        "risk_check",
        serde_json::json!({
            "passed": !report.has_failures(),
            "checks": check_data,
        }),
    )
}

/// Convenience: log risk check passed as a checkpoint.
pub fn log_risk_check_passed_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    report: &crate::risk::RiskReport,
) -> Result<()> {
    let check_data: Vec<_> = report
        .checks
        .iter()
        .map(|c| {
            serde_json::json!({
                "name": c.name,
                "status": format!("{}", c.status),
                "detail": c.detail,
            })
        })
        .collect();

    audit.log_checkpoint(
        Checkpoint::RiskCheckPassed,
        sequence_number,
        serde_json::json!({
            "checks": check_data,
        }),
    )
}

/// Convenience: log order submission.
pub fn log_order_submitted(
    audit: &mut AuditLog,
    order: &crate::diff::RebalanceOrder,
    ibkr_id: i32,
) -> Result<()> {
    audit.log(
        "order_submitted",
        serde_json::json!({
            "symbol": order.symbol.as_str(),
            "action": format!("{}", order.action),
            "shares": order.shares,
            "limit": order.limit_price_cents as f64 / 100.0,
            "ibkr_id": ibkr_id,
        }),
    )
}

/// Convenience: log order submission as a checkpoint.
pub fn log_order_submitted_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    order: &crate::diff::RebalanceOrder,
    ibkr_id: i32,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::OrderSubmitted,
        sequence_number,
        serde_json::json!({
            "symbol": order.symbol.as_str(),
            "action": format!("{}", order.action),
            "shares": order.shares,
            "limit": order.limit_price_cents as f64 / 100.0,
            "ibkr_id": ibkr_id,
        }),
    )
}

/// Convenience: log order fill.
pub fn log_order_filled(
    audit: &mut AuditLog,
    result: &nanobook_broker::ibkr::orders::OrderResult,
) -> Result<()> {
    audit.log(
        "order_filled",
        serde_json::json!({
            "symbol": result.symbol.as_str(),
            "ibkr_id": result.order_id,
            "filled": result.filled_shares,
            "avg_price": result.avg_fill_price,
            "commission": result.commission,
            "status": format!("{:?}", result.status),
        }),
    )
}

/// Convenience: log order fill as a checkpoint.
pub fn log_order_filled_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    result: &nanobook_broker::ibkr::orders::OrderResult,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::OrderFilled,
        sequence_number,
        serde_json::json!({
            "symbol": result.symbol.as_str(),
            "ibkr_id": result.order_id,
            "filled": result.filled_shares,
            "avg_price": result.avg_fill_price,
            "commission": result.commission,
            "status": format!("{:?}", result.status),
        }),
    )
}

/// Convenience: log run completion.
pub fn log_run_completed(
    audit: &mut AuditLog,
    submitted: usize,
    filled: usize,
    failed: usize,
) -> Result<()> {
    audit.log(
        "run_completed",
        serde_json::json!({
            "submitted": submitted,
            "filled": filled,
            "failed": failed,
        }),
    )
}

/// Convenience: log run completion as a checkpoint.
pub fn log_run_completed_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    submitted: usize,
    filled: usize,
    failed: usize,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::RunCompleted,
        sequence_number,
        serde_json::json!({
            "submitted": submitted,
            "filled": filled,
            "failed": failed,
        }),
    )
}

/// Convenience: log cron mode start with sequence number.
pub fn log_cron_start(
    audit: &mut AuditLog,
    sequence_number: u64,
    window_id: &str,
) -> Result<()> {
    audit.log_with_idempotency(
        "cron_start",
        Some(sequence_number),
        Some(window_id.to_string()),
        serde_json::json!({}),
    )
}

/// Convenience: log cron mode completion.
pub fn log_cron_completed(
    audit: &mut AuditLog,
    sequence_number: u64,
    window_id: &str,
    submitted: usize,
    filled: usize,
    failed: usize,
) -> Result<()> {
    audit.log_with_idempotency(
        "cron_completed",
        Some(sequence_number),
        Some(window_id.to_string()),
        serde_json::json!({
            "submitted": submitted,
            "filled": filled,
            "failed": failed,
        }),
    )
}

/// Convenience: log idempotency rejection.
pub fn log_idempotency_rejection(
    audit: &mut AuditLog,
    window_id: &str,
    existing_sequence: u64,
) -> Result<()> {
    audit.log_with_idempotency(
        "idempotency_rejection",
        Some(existing_sequence),
        Some(window_id.to_string()),
        serde_json::json!({}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_log_writes_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_audit.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_simple("test_event").unwrap();
            log.log("test_data", serde_json::json!({"key": "value"}))
                .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        // Each line should be valid JSON
        for line in &lines {
            let _: serde_json::Value = serde_json::from_str(line).unwrap();
        }

        // First line should have "test_event"
        assert!(lines[0].contains("\"event\":\"test_event\""));
    }

    #[test]
    fn audit_log_creates_parent_dirs() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("subdir").join("deep").join("audit.jsonl");

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_simple("test").unwrap();

        assert!(path.exists());
    }

    #[test]
    fn checkpoint_logging_with_sequence_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_checkpoint.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(
                Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::PositionsFetched,
                2,
                serde_json::json!({"equity": 100000}),
            ).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 2);

        // Verify checkpoint field is present
        let event1: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event1.event, "run_started");
        assert_eq!(event1.sequence_number, Some(1));
        assert!(event1.checkpoint.is_some());

        let event2: AuditEvent = serde_json::from_str(lines[1]).unwrap();
        assert_eq!(event2.event, "positions_fetched");
        assert_eq!(event2.sequence_number, Some(2));
        assert!(event2.checkpoint.is_some());
    }

    #[test]
    fn checkpoint_validation_monotonic_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({})).unwrap();
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({})).unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 3, serde_json::json!({})).unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 4, serde_json::json!({})).unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    #[test]
    fn checkpoint_validation_non_monotonic_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_bad.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({})).unwrap();
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({})).unwrap();
            // Write a checkpoint with non-monotonic sequence number
            let event = AuditEvent {
                event: "diff_computed".to_string(),
                ts: chrono::Utc::now(),
                sequence_number: Some(1), // Should be > 2, not < 2
                window_id: None,
                checkpoint: Some("DiffComputed".to_string()),
                data: serde_json::json!({}),
            };
            let json = serde_json::to_string(&event).unwrap();
            std::fs::write(&path, std::fs::read_to_string(&path).unwrap() + &json).unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_err());
    }

    #[test]
    fn checkpoint_validation_missing_sequence_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_missing_seq.jsonl");

        // Write a checkpoint without sequence number
        let event = AuditEvent {
            event: "run_started".to_string(),
            ts: chrono::Utc::now(),
            sequence_number: None, // Missing sequence number
            window_id: None,
            checkpoint: Some("RunStarted".to_string()),
            data: serde_json::json!({}),
        };
        let json = serde_json::to_string(&event).unwrap();
        std::fs::write(&path, json).unwrap();

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_err());
    }

    // ========================================================================
    // Sandboxing (S8)
    // ========================================================================

    /// A nonexistent audit directory under `workdir` is accepted —
    /// the canonicalize-as-far-as-possible walk keeps the validation
    /// useful on first run when `logs/` doesn't exist yet.
    #[test]
    fn sandboxing_accepts_nonexistent_path_under_workdir() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("does").join("not").join("exist.jsonl");

        assert!(
            AuditLog::open_in(&path, dir.path()).is_ok(),
            "nonexistent path under workdir must be accepted"
        );
    }

    /// An absolute path pointing outside `workdir` is rejected with
    /// `AuditPathOutsideWorkdir`, not silently accepted.
    #[test]
    fn sandboxing_rejects_absolute_path_outside_workdir() {
        let workdir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let path = outside.path().join("audit.jsonl");

        let err = AuditLog::open_in(&path, workdir.path())
            .expect_err("path outside workdir must be rejected");
        assert!(
            matches!(err, Error::AuditPathOutsideWorkdir { .. }),
            "expected AuditPathOutsideWorkdir, got {err:?}",
        );
    }

    /// Parent-traversal attempts like `workdir/../elsewhere` are
    /// rejected because `canonicalize` resolves `..` before the
    /// `starts_with` check.
    #[test]
    fn sandboxing_rejects_parent_traversal() {
        let workdir = tempfile::tempdir().unwrap();
        // Create an existing sibling directory that ../ would escape to.
        let sibling = workdir.path().parent().unwrap().join("sibling-escape");
        let _ = std::fs::create_dir_all(&sibling);
        let path = sibling.join("audit.jsonl");

        let err = AuditLog::open_in(&path, workdir.path())
            .expect_err("parent-traversal must be rejected");
        assert!(matches!(err, Error::AuditPathOutsideWorkdir { .. }));

        let _ = std::fs::remove_dir_all(&sibling);
    }

    /// On Unix, a symlink inside `workdir` that points to a location
    /// outside `workdir` is rejected — this is the primary attack
    /// S8 closes. Not run on Windows: the stdlib's `canonicalize` on
    /// Windows uses a different semantic (UNC long paths), and
    /// symlinks require privileged dev-mode creation.
    #[cfg(unix)]
    #[test]
    fn sandboxing_rejects_symlink_escape() {
        let workdir = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();

        // Create `workdir/logs -> outside/`.
        let symlink_path = workdir.path().join("logs");
        std::os::unix::fs::symlink(outside.path(), &symlink_path).unwrap();

        let audit_path = symlink_path.join("audit.jsonl");
        let err = AuditLog::open_in(&audit_path, workdir.path())
            .expect_err("symlink escape must be rejected");
        assert!(matches!(err, Error::AuditPathOutsideWorkdir { .. }));
    }

    /// Regression for S7: on Unix, a newly-created audit file must
    /// have mode `0o600` (owner read/write only). The low 9 bits
    /// are the permission bits; higher bits encode file type and
    /// sticky/setuid flags and should be masked off before the
    /// comparison.
    #[cfg(unix)]
    #[test]
    fn audit_log_is_0o600_on_unix() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_simple("test").unwrap();
        drop(log); // flush and close

        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "expected audit file mode 0o600, got {:o}",
            mode & 0o777,
        );
    }

    // ========================================================================
    // Clock Skew Detection (F5)
    // ========================================================================

    /// Clock skew detector is initialized when audit log is created.
    #[test]
    fn clock_skew_detector_initialized() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // First log should work (detector accepts first timestamp)
        log.log_simple("test_event").unwrap();
        drop(log);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"event\":\"test_event\""));
    }

    /// Backward jump detection works with audit log integration.
    #[test]
    fn backward_jump_detection_integration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Create detector with low threshold for testing
        let mut detector = ClockSkewDetector::with_thresholds(5, 2.0);
        let past_ts = Utc::now() - chrono::Duration::seconds(10);
        detector.set_last_timestamp(past_ts);

        let mut log = AuditLog::open_in_with_detector(&path, dir.path(), detector).unwrap();
        // Log should succeed even though detector will detect skew
        log.log_simple("test_event").unwrap();
        drop(log);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"event\":\"test_event\""));
        // Logging continued despite skew
    }

    /// Forward jump detection works with audit log integration.
    #[test]
    fn forward_jump_detection_integration() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Create detector with low threshold for testing
        let mut detector = ClockSkewDetector::with_thresholds(5, 2.0);
        let past_ts = Utc::now() - chrono::Duration::seconds(100);
        detector.set_last_timestamp(past_ts);

        let mut log = AuditLog::open_in_with_detector(&path, dir.path(), detector).unwrap();
        // Log should succeed even though detector will detect skew
        log.log_simple("test_event").unwrap();
        drop(log);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"event\":\"test_event\""));
        // Logging continued despite skew
    }

    /// Audit log continues to work after clock skew is detected.
    #[test]
    fn logging_continues_despite_skew() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_simple("test_event").unwrap();
        drop(log);

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"event\":\"test_event\""));
    }

    // ========================================================================
    // Cron Mode Idempotency (F7)
    // ========================================================================

    /// check_window_already_complete returns None when window not found.
    #[test]
    fn check_window_not_complete_returns_none() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        let result = log.check_window_already_complete("test-window").unwrap();
        assert_eq!(result, None);
    }

    /// check_window_already_complete returns sequence number when window is complete.
    #[test]
    fn check_window_complete_returns_sequence_number() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Log a cron_completed event
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_completed",
                Some(123),
                Some("test-window".to_string()),
                serde_json::json!({"submitted": 5, "filled": 5, "failed": 0}),
            )
            .unwrap();
        }

        // Check if window is complete
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        let result = log.check_window_already_complete("test-window").unwrap();
        assert_eq!(result, Some(123));
    }

    /// check_window_already_complete handles multiple windows independently.
    #[test]
    fn check_window_multiple_windows_independent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Log completion for window A
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_completed",
                Some(100),
                Some("window-a".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
        }

        // Log completion for window B
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_completed",
                Some(200),
                Some("window-b".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
        }

        // Check window A
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        let result_a = log.check_window_already_complete("window-a").unwrap();
        assert_eq!(result_a, Some(100));

        // Check window B
        let result_b = log.check_window_already_complete("window-b").unwrap();
        assert_eq!(result_b, Some(200));

        // Check non-existent window
        let result_c = log.check_window_already_complete("window-c").unwrap();
        assert_eq!(result_c, None);
    }

    /// check_window_already_complete ignores non-cron_completed events.
    #[test]
    fn check_window_ignores_other_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Log various events with window_id but not cron_completed
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_start",
                Some(123),
                Some("test-window".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
            log.log_with_idempotency(
                "run_started",
                Some(123),
                Some("test-window".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
        }

        // Window should not be considered complete
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        let result = log.check_window_already_complete("test-window").unwrap();
        assert_eq!(result, None);
    }

    /// check_window_already_complete returns the most recent completion.
    #[test]
    fn check_window_returns_most_recent_completion() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("audit.jsonl");

        // Log first completion
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_completed",
                Some(100),
                Some("test-window".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
        }

        // Log second completion (e.g., retry with different sequence)
        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_with_idempotency(
                "cron_completed",
                Some(200),
                Some("test-window".to_string()),
                serde_json::json!({}),
            )
            .unwrap();
        }

        // Should return the most recent (last in file)
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        let result = log.check_window_already_complete("test-window").unwrap();
        assert_eq!(result, Some(200));
    }
}
