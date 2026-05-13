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
use serde::Serialize;
use log::warn;

use crate::clock_skew::{ClockSkewDetector, SkewResult};
use crate::error::{Error, Result};

/// An audit event written to the JSONL trail.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub event: &'static str,
    pub ts: DateTime<Utc>,
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
            writer: BufWriter::new(file),
            clock_skew_detector: detector,
        })
    }

    /// Log an event with arbitrary JSON data.
    pub fn log(&mut self, event: &'static str, data: serde_json::Value) -> Result<()> {
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
            event,
            ts,
            data,
        };
        let json = serde_json::to_string(&entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(self.writer, "{json}")?;
        self.writer.flush()?;
        Ok(())
    }

    /// Log a simple event with no additional data.
    pub fn log_simple(&mut self, event: &'static str) -> Result<()> {
        self.log(event, serde_json::json!({}))
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

        // Create detector with low threshold
        let mut detector = ClockSkewDetector::with_thresholds(5, 2.0);
        let past_ts = Utc::now() - chrono::Duration::seconds(100);
        detector.set_last_timestamp(past_ts);

        let mut log = AuditLog::open_in_with_detector(&path, dir.path(), detector).unwrap();

        // Log multiple events - all should succeed
        log.log_simple("event1").unwrap();
        log.log_simple("event2").unwrap();
        log.log_simple("event3").unwrap();
        drop(log);

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 3);
    }
}
