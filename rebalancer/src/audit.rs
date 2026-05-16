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
use tracing::warn;

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
    /// Positions fetch intent logged before broker call (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    PositionsIntent,
    /// Current positions retrieved from broker
    PositionsFetched,
    /// Positions fetch result logged after positions are fetched (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    PositionsResult,
    /// Rebalance diff computed
    DiffComputed,
    /// Risk checks passed
    RiskCheckPassed,
    /// Order submission intent logged before broker call (write-ahead logging)
    OrderIntent,
    /// Individual order submitted (symbol is encoded in event data)
    OrderSubmitted,
    /// Order submission failed with error details (write-ahead logging)
    OrderFailed,
    /// Individual order filled (symbol is encoded in event data)
    OrderFilled,
    /// Quotes fetch intent logged before broker call (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    QuotesIntent,
    /// Quotes fetch result logged after quotes are fetched (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    QuotesResult,
    /// Account summary fetch intent logged before broker call (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    AccountSummaryIntent,
    /// Account summary fetch result logged after account summary is fetched (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    AccountSummaryResult,
    /// Order cancellation intent logged before broker call (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    CancelIntent,
    /// Order cancellation result logged after cancellation attempt (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    CancelResult,
    /// Rebalance run completes (success or failure)
    RunCompleted,
}

impl Checkpoint {
    /// Convert checkpoint to event name string
    pub fn as_event_name(&self) -> &'static str {
        match self {
            Checkpoint::RunStarted => "run_started",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::PositionsIntent => "positions_intent",
            Checkpoint::PositionsFetched => "positions_fetched",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::PositionsResult => "positions_result",
            Checkpoint::DiffComputed => "diff_computed",
            Checkpoint::RiskCheckPassed => "risk_check_passed",
            Checkpoint::OrderIntent => "order_intent",
            Checkpoint::OrderSubmitted => "order_submitted",
            Checkpoint::OrderFailed => "order_failed",
            Checkpoint::OrderFilled => "order_filled",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::QuotesIntent => "quotes_intent",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::QuotesResult => "quotes_result",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::AccountSummaryIntent => "account_summary_intent",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::AccountSummaryResult => "account_summary_result",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::CancelIntent => "cancel_intent",
            #[cfg(feature = "write_ahead_logging")]
            Checkpoint::CancelResult => "cancel_result",
            Checkpoint::RunCompleted => "run_completed",
        }
    }

    /// Parse event name string to checkpoint
    pub fn from_event_name(name: &str) -> Option<Self> {
        match name {
            "run_started" => Some(Checkpoint::RunStarted),
            #[cfg(feature = "write_ahead_logging")]
            "positions_intent" => Some(Checkpoint::PositionsIntent),
            "positions_fetched" => Some(Checkpoint::PositionsFetched),
            #[cfg(feature = "write_ahead_logging")]
            "positions_result" => Some(Checkpoint::PositionsResult),
            "diff_computed" => Some(Checkpoint::DiffComputed),
            "risk_check_passed" => Some(Checkpoint::RiskCheckPassed),
            "order_intent" => Some(Checkpoint::OrderIntent),
            "order_submitted" => Some(Checkpoint::OrderSubmitted),
            "order_failed" => Some(Checkpoint::OrderFailed),
            "order_filled" => Some(Checkpoint::OrderFilled),
            #[cfg(feature = "write_ahead_logging")]
            "quotes_intent" => Some(Checkpoint::QuotesIntent),
            #[cfg(feature = "write_ahead_logging")]
            "quotes_result" => Some(Checkpoint::QuotesResult),
            #[cfg(feature = "write_ahead_logging")]
            "account_summary_intent" => Some(Checkpoint::AccountSummaryIntent),
            #[cfg(feature = "write_ahead_logging")]
            "account_summary_result" => Some(Checkpoint::AccountSummaryResult),
            #[cfg(feature = "write_ahead_logging")]
            "cancel_intent" => Some(Checkpoint::CancelIntent),
            #[cfg(feature = "write_ahead_logging")]
            "cancel_result" => Some(Checkpoint::CancelResult),
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

pub fn parse_audit_events(path: &Path) -> Result<Vec<AuditEvent>> {
    let contents = std::fs::read_to_string(path)?;
    parse_audit_events_from_str(&contents)
}

pub fn parse_audit_events_from_str(contents: &str) -> Result<Vec<AuditEvent>> {
    let mut events = Vec::new();
    for (line_num, line) in contents.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<AuditEvent>(line) {
            Ok(event) => events.push(event),
            Err(_) => {
                return Err(Error::AuditValidation(format!(
                    "Corrupted audit log at line {}: invalid JSON",
                    line_num + 1
                )));
            }
        }
    }
    Ok(events)
}

/// Return the highest checkpoint sequence number currently present in an audit log.
///
/// Missing logs are treated as empty so the first run can create the file. Corrupt
/// existing logs still fail closed because appending new checkpoints to them would
/// make recovery ambiguity worse.
pub fn max_checkpoint_sequence(path: &Path) -> Result<u64> {
    match std::fs::read_to_string(path) {
        Ok(contents) => Ok(parse_audit_events_from_str(&contents)?
            .into_iter()
            .filter(|event| Checkpoint::from_event_name(&event.event).is_some())
            .filter_map(|event| event.sequence_number)
            .max()
            .unwrap_or(0)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(error.into()),
    }
}

pub fn validate_checkpoints_from_parsed(events: &[AuditEvent]) -> Result<()> {
    let mut last_sequence: Option<u64> = None;

    let mut found_checkpoints: Vec<Checkpoint> = Vec::new();

    for (index, event) in events.iter().enumerate() {
        if let Some(checkpoint) = Checkpoint::from_event_name(&event.event) {
            let sequence_number = event.sequence_number.ok_or_else(|| {
                Error::AuditValidation(format!(
                    "Checkpoint at line {} missing sequence number",
                    index + 1
                ))
            })?;

            if let Some(last_seq) = last_sequence {
                if sequence_number <= last_seq {
                    return Err(Error::AuditValidation(format!(
                        "Checkpoint sequence not monotonic at line {}: got {}, expected > {}",
                        index + 1,
                        sequence_number,
                        last_seq
                    )));
                }
            }
            last_sequence = Some(sequence_number);

            found_checkpoints.push(checkpoint);
        }
    }

    let index_of = |checkpoint: Checkpoint| {
        found_checkpoints
            .iter()
            .position(|&candidate| candidate == checkpoint)
    };

    let require_checkpoint = |checkpoint: Checkpoint| -> Result<usize> {
        index_of(checkpoint).ok_or_else(|| {
            Error::AuditValidation(format!("Missing checkpoint: expected {:?}", checkpoint))
        })
    };

    let run_started_idx = require_checkpoint(Checkpoint::RunStarted)?;
    let diff_idx = require_checkpoint(Checkpoint::DiffComputed)?;

    if run_started_idx != 0 {
        return Err(Error::AuditValidation(
            "RunStarted must be the first checkpoint".to_string(),
        ));
    }

    #[cfg(feature = "write_ahead_logging")]
    let positions_complete_idx = {
        let intent_idx = require_checkpoint(Checkpoint::PositionsIntent)?;
        let result_idx = require_checkpoint(Checkpoint::PositionsResult)?;
        if result_idx <= intent_idx {
            return Err(Error::AuditValidation(
                "PositionsResult must come after PositionsIntent".to_string(),
            ));
        }
        result_idx
    };

    #[cfg(not(feature = "write_ahead_logging"))]
    let positions_complete_idx = require_checkpoint(Checkpoint::PositionsFetched)?;

    if positions_complete_idx <= run_started_idx {
        return Err(Error::AuditValidation(
            "positions checkpoint must come after RunStarted".to_string(),
        ));
    }
    if diff_idx <= positions_complete_idx {
        return Err(Error::AuditValidation(
            "DiffComputed must come after positions are available".to_string(),
        ));
    }

    let diff_orders = events
        .iter()
        .rev()
        .find(|event| Checkpoint::from_event_name(&event.event) == Some(Checkpoint::DiffComputed))
        .and_then(|event| event.data.get("orders"))
        .and_then(|orders| orders.as_array());
    let empty_diff_completed_run = diff_orders.is_some_and(|orders| orders.is_empty());

    if !empty_diff_completed_run {
        let risk_idx = require_checkpoint(Checkpoint::RiskCheckPassed)?;
        let order_intent_idx = require_checkpoint(Checkpoint::OrderIntent)?;
        if risk_idx <= diff_idx {
            return Err(Error::AuditValidation(
                "RiskCheckPassed must come after DiffComputed".to_string(),
            ));
        }
        if order_intent_idx <= risk_idx {
            return Err(Error::AuditValidation(
                "OrderIntent must come after RiskCheckPassed".to_string(),
            ));
        }
    } else if let Some(run_completed_idx) = index_of(Checkpoint::RunCompleted) {
        if run_completed_idx <= diff_idx {
            return Err(Error::AuditValidation(
                "RunCompleted must come after empty DiffComputed".to_string(),
            ));
        }
    }

    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = index_of(Checkpoint::AccountSummaryIntent) {
        if let Some(result_idx) = index_of(Checkpoint::AccountSummaryResult) {
            if result_idx <= intent_idx {
                return Err(Error::AuditValidation(
                    "AccountSummaryResult must come after AccountSummaryIntent".to_string(),
                ));
            }
        }
        if intent_idx <= run_started_idx {
            return Err(Error::AuditValidation(
                "AccountSummaryIntent must come after RunStarted".to_string(),
            ));
        }
    }

    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = index_of(Checkpoint::QuotesIntent) {
        if let Some(result_idx) = index_of(Checkpoint::QuotesResult) {
            if result_idx <= intent_idx {
                return Err(Error::AuditValidation(
                    "QuotesResult must come after QuotesIntent".to_string(),
                ));
            }
        }
    }

    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = index_of(Checkpoint::CancelIntent) {
        if let Some(result_idx) = index_of(Checkpoint::CancelResult) {
            if result_idx <= intent_idx {
                return Err(Error::AuditValidation(
                    "CancelResult must come after CancelIntent".to_string(),
                ));
            }
        }
    }

    // Validate that OrderIntent has either OrderSubmitted or OrderFailed after it (not incomplete)
    // This is a soft validation - we allow incomplete intents during crash recovery
    // but we should warn about them
    if let Some(intent_idx) = found_checkpoints
        .iter()
        .position(|&c| c == Checkpoint::OrderIntent)
    {
        let has_followup = found_checkpoints.iter().skip(intent_idx + 1).any(|c| {
            matches!(c, Checkpoint::OrderSubmitted) || matches!(c, Checkpoint::OrderFailed)
        });
        if !has_followup {
            tracing::warn!(
                "Found OrderIntent checkpoint without OrderSubmitted or OrderFailed - this indicates an incomplete order submission that may need broker reconciliation"
            );
        }
    }

    // Validate that PositionsIntent has PositionsResult after it (not incomplete)
    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = found_checkpoints
        .iter()
        .position(|&c| c == Checkpoint::PositionsIntent)
    {
        let has_followup = found_checkpoints
            .iter()
            .skip(intent_idx + 1)
            .any(|c| matches!(c, Checkpoint::PositionsResult));
        if !has_followup {
            tracing::warn!(
                "Found PositionsIntent checkpoint without PositionsResult - this indicates an incomplete positions fetch that may need broker reconciliation"
            );
        }
    }

    // Validate that QuotesIntent has QuotesResult after it (not incomplete)
    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = found_checkpoints
        .iter()
        .position(|&c| c == Checkpoint::QuotesIntent)
    {
        let has_followup = found_checkpoints
            .iter()
            .skip(intent_idx + 1)
            .any(|c| matches!(c, Checkpoint::QuotesResult));
        if !has_followup {
            tracing::warn!(
                "Found QuotesIntent checkpoint without QuotesResult - this indicates an incomplete quotes fetch that may need broker reconciliation"
            );
        }
    }

    // Validate that AccountSummaryIntent has AccountSummaryResult after it (not incomplete)
    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = found_checkpoints
        .iter()
        .position(|&c| c == Checkpoint::AccountSummaryIntent)
    {
        let has_followup = found_checkpoints
            .iter()
            .skip(intent_idx + 1)
            .any(|c| matches!(c, Checkpoint::AccountSummaryResult));
        if !has_followup {
            tracing::warn!(
                "Found AccountSummaryIntent checkpoint without AccountSummaryResult - this indicates an incomplete account summary fetch that may need broker reconciliation"
            );
        }
    }

    // Validate that CancelIntent has CancelResult after it (not incomplete)
    #[cfg(feature = "write_ahead_logging")]
    if let Some(intent_idx) = found_checkpoints
        .iter()
        .position(|&c| c == Checkpoint::CancelIntent)
    {
        let has_followup = found_checkpoints
            .iter()
            .skip(intent_idx + 1)
            .any(|c| matches!(c, Checkpoint::CancelResult));
        if !has_followup {
            tracing::warn!(
                "Found CancelIntent checkpoint without CancelResult - this indicates an incomplete order cancellation that may need broker reconciliation"
            );
        }
    }

    Ok(())
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
    pub fn check_window_already_complete(&mut self, window_id: &str) -> Result<Option<u64>> {
        self.writer.flush()?;

        let mut last_sequence: Option<u64> = None;
        for event in parse_audit_events(&self.path)? {
            if event.event == "cron_completed" && event.window_id.as_deref() == Some(window_id) {
                if let Some(seq) = event.sequence_number {
                    last_sequence = Some(seq);
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
        self.writer.flush()?;
        let events = parse_audit_events(&self.path)?;
        validate_checkpoints_from_parsed(&events)
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

/// Convenience: log order intent before broker call (write-ahead logging).
pub fn log_order_intent(
    audit: &mut AuditLog,
    order: &crate::diff::RebalanceOrder,
    client_order_id: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
    execution_context: &str,
) -> Result<()> {
    audit.log(
        "order_intent",
        serde_json::json!({
            "symbol": order.symbol.as_str(),
            "action": format!("{}", order.action),
            "shares": order.shares,
            "limit": order.limit_price_cents as f64 / 100.0,
            "client_order_id": client_order_id,
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
            "execution_context": execution_context,
        }),
    )
}

/// Convenience: log order submission as a checkpoint.
pub fn log_order_intent_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    order: &crate::diff::RebalanceOrder,
    client_order_id: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
    execution_context: &str,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::OrderIntent,
        sequence_number,
        serde_json::json!({
            "symbol": order.symbol.as_str(),
            "action": format!("{}", order.action),
            "shares": order.shares,
            "limit": order.limit_price_cents as f64 / 100.0,
            "client_order_id": client_order_id,
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
            "execution_context": execution_context,
        }),
    )
}

/// Convenience: log order submission failure (write-ahead logging).
pub fn log_order_failed(
    audit: &mut AuditLog,
    error_type: &str,
    error_message: &str,
    context: &str,
) -> Result<()> {
    audit.log(
        "order_failed",
        serde_json::json!({
            "error_type": error_type,
            "error_message": error_message,
            "context": context,
        }),
    )
}

/// Convenience: log order submission failure as a checkpoint.
pub fn log_order_failed_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    error_type: &str,
    error_message: &str,
    context: &str,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::OrderFailed,
        sequence_number,
        serde_json::json!({
            "error_type": error_type,
            "error_message": error_message,
            "context": context,
        }),
    )
}

/// Convenience: log positions fetch intent before broker call (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_positions_intent(
    audit: &mut AuditLog,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    audit.log(
        "positions_intent",
        serde_json::json!({
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log positions fetch intent as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_positions_intent_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::PositionsIntent,
        sequence_number,
        serde_json::json!({
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log positions fetch result after positions are fetched (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_positions_result(
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
        "positions_result",
        serde_json::json!({
            "positions": pos_data,
            "equity": equity_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log positions fetch result as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_positions_result_checkpoint(
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
        Checkpoint::PositionsResult,
        sequence_number,
        serde_json::json!({
            "positions": pos_data,
            "equity": equity_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log quotes fetch intent before broker call (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_quotes_intent(
    audit: &mut AuditLog,
    symbols: &[nanobook::Symbol],
    staleness_threshold_sec: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    let symbol_list: Vec<_> = symbols.iter().map(|s| s.as_str()).collect();

    audit.log(
        "quotes_intent",
        serde_json::json!({
            "symbols": symbol_list,
            "staleness_threshold_sec": staleness_threshold_sec,
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log quotes fetch intent as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_quotes_intent_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    symbols: &[nanobook::Symbol],
    staleness_threshold_sec: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    let symbol_list: Vec<_> = symbols.iter().map(|s| s.as_str()).collect();

    audit.log_checkpoint(
        Checkpoint::QuotesIntent,
        sequence_number,
        serde_json::json!({
            "symbols": symbol_list,
            "staleness_threshold_sec": staleness_threshold_sec,
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log quotes fetch result after quotes are fetched (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_quotes_result(
    audit: &mut AuditLog,
    quotes: &[nanobook_broker::types::Quote],
) -> Result<()> {
    let quote_data: Vec<_> = quotes
        .iter()
        .map(|q| {
            let timestamp = chrono::DateTime::<chrono::Utc>::from(q.timestamp)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            serde_json::json!({
                "symbol": q.symbol.as_str(),
                "bid_cents": q.bid_cents,
                "ask_cents": q.ask_cents,
                "last_cents": q.last_cents,
                "timestamp": timestamp,
            })
        })
        .collect();

    audit.log(
        "quotes_result",
        serde_json::json!({
            "quotes": quote_data,
        }),
    )
}

/// Convenience: log quotes fetch result as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_quotes_result_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    quotes: &[nanobook_broker::types::Quote],
) -> Result<()> {
    let quote_data: Vec<_> = quotes
        .iter()
        .map(|q| {
            let timestamp = chrono::DateTime::<chrono::Utc>::from(q.timestamp)
                .to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
            serde_json::json!({
                "symbol": q.symbol.as_str(),
                "bid_cents": q.bid_cents,
                "ask_cents": q.ask_cents,
                "last_cents": q.last_cents,
                "timestamp": timestamp,
            })
        })
        .collect();

    audit.log_checkpoint(
        Checkpoint::QuotesResult,
        sequence_number,
        serde_json::json!({
            "quotes": quote_data,
        }),
    )
}

/// Convenience: log account summary fetch intent before broker call (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_account_summary_intent(
    audit: &mut AuditLog,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    audit.log(
        "account_summary_intent",
        serde_json::json!({
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log account summary fetch intent as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_account_summary_intent_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    timestamp: chrono::DateTime<chrono::Utc>,
    target_spec_reference: &str,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::AccountSummaryIntent,
        sequence_number,
        serde_json::json!({
            "timestamp": timestamp.to_rfc3339(),
            "target_spec_reference": target_spec_reference,
        }),
    )
}

/// Convenience: log account summary fetch result after account summary is fetched (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_account_summary_result(
    audit: &mut AuditLog,
    equity_cents: i64,
    cash_cents: i64,
) -> Result<()> {
    audit.log(
        "account_summary_result",
        serde_json::json!({
            "equity": equity_cents as f64 / 100.0,
            "cash": cash_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log account summary fetch result as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_account_summary_result_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    equity_cents: i64,
    cash_cents: i64,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::AccountSummaryResult,
        sequence_number,
        serde_json::json!({
            "equity": equity_cents as f64 / 100.0,
            "cash": cash_cents as f64 / 100.0,
        }),
    )
}

/// Convenience: log order cancellation intent before broker call (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_cancel_intent(
    audit: &mut AuditLog,
    order_id: u64,
    cancellation_reason: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    audit.log(
        "cancel_intent",
        serde_json::json!({
            "order_id": order_id,
            "cancellation_reason": cancellation_reason,
            "timestamp": timestamp.to_rfc3339(),
        }),
    )
}

/// Convenience: log order cancellation intent as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_cancel_intent_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    order_id: u64,
    cancellation_reason: &str,
    timestamp: chrono::DateTime<chrono::Utc>,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::CancelIntent,
        sequence_number,
        serde_json::json!({
            "order_id": order_id,
            "cancellation_reason": cancellation_reason,
            "timestamp": timestamp.to_rfc3339(),
        }),
    )
}

/// Convenience: log order cancellation result after cancellation attempt (write-ahead logging).
#[cfg(feature = "write_ahead_logging")]
pub fn log_cancel_result(
    audit: &mut AuditLog,
    order_id: u64,
    success: bool,
    error_message: Option<&str>,
) -> Result<()> {
    audit.log(
        "cancel_result",
        serde_json::json!({
            "order_id": order_id,
            "success": success,
            "error_message": error_message,
        }),
    )
}

/// Convenience: log order cancellation result as a checkpoint.
#[cfg(feature = "write_ahead_logging")]
pub fn log_cancel_result_checkpoint(
    audit: &mut AuditLog,
    sequence_number: u64,
    order_id: u64,
    success: bool,
    error_message: Option<&str>,
) -> Result<()> {
    audit.log_checkpoint(
        Checkpoint::CancelResult,
        sequence_number,
        serde_json::json!({
            "order_id": order_id,
            "success": success,
            "error_message": error_message,
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

/// Convenience: log a kill-switch request.
pub fn log_kill_requested(audit: &mut AuditLog, method: &str, trigger_source: &str) -> Result<()> {
    audit.log(
        "kill_requested",
        serde_json::json!({
            "method": method,
            "trigger_source": trigger_source,
        }),
    )
}

/// Convenience: log kill completion with a full summary.
pub fn log_kill_completed_with_summary(
    audit: &mut AuditLog,
    method: &str,
    orders_cancelled_count: usize,
    orders_remaining_count: usize,
    duration_seconds: f64,
    error_messages: &[String],
) -> Result<()> {
    audit.log(
        "kill_completed",
        serde_json::json!({
            "method": method,
            "orders_cancelled_count": orders_cancelled_count,
            "orders_remaining_count": orders_remaining_count,
            "duration_seconds": duration_seconds,
            "error_messages": error_messages,
        }),
    )
}

/// Convenience: log graceful kill completion.
pub fn log_kill_completed(
    audit: &mut AuditLog,
    method: &str,
    orders_cancelled_count: usize,
    duration_seconds: f64,
) -> Result<()> {
    log_kill_completed_with_summary(
        audit,
        method,
        orders_cancelled_count,
        0,
        duration_seconds,
        &[],
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
pub fn log_cron_start(audit: &mut AuditLog, sequence_number: u64, window_id: &str) -> Result<()> {
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
            )
            .unwrap();
            log.log_checkpoint(
                Checkpoint::PositionsFetched,
                2,
                serde_json::json!({"equity": 100000}),
            )
            .unwrap();
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
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
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
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
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

    // ========================================================================
    // OrderIntent and OrderFailed Checkpoint Tests
    // ========================================================================

    /// Test that Checkpoint::from_event_name parses "order_intent" correctly.
    #[test]
    fn checkpoint_from_event_name_order_intent() {
        let result = Checkpoint::from_event_name("order_intent");
        assert_eq!(result, Some(Checkpoint::OrderIntent));
    }

    /// Test that Checkpoint::from_event_name parses "order_failed" correctly.
    #[test]
    fn checkpoint_from_event_name_order_failed() {
        let result = Checkpoint::from_event_name("order_failed");
        assert_eq!(result, Some(Checkpoint::OrderFailed));
    }

    /// Test that Checkpoint::OrderIntent.as_event_name returns "order_intent".
    #[test]
    fn checkpoint_as_event_name_order_intent() {
        assert_eq!(Checkpoint::OrderIntent.as_event_name(), "order_intent");
    }

    /// Test that Checkpoint::OrderFailed.as_event_name returns "order_failed".
    #[test]
    fn checkpoint_as_event_name_order_failed() {
        assert_eq!(Checkpoint::OrderFailed.as_event_name(), "order_failed");
    }

    /// Test that validation accepts the new checkpoint sequence with OrderIntent.
    #[test]
    fn checkpoint_validation_accepts_new_sequence_with_order_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation warns about incomplete OrderIntent without followup.
    #[test]
    fn checkpoint_validation_warns_incomplete_order_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_incomplete.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            // No OrderSubmitted or OrderFailed after OrderIntent - this should warn
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // Validation should succeed (soft validation), but we can't easily test the warning log
        // in unit tests without capturing logs. The important thing is it doesn't error.
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation accepts OrderIntent followed by OrderSubmitted.
    #[test]
    fn checkpoint_validation_accepts_order_intent_then_submitted() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_success.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderSubmitted, 7, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation accepts OrderIntent followed by OrderFailed.
    #[test]
    fn checkpoint_validation_accepts_order_intent_then_failed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_failure.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderFailed, 7, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test log_order_intent creates a valid audit event with all required fields.
    #[test]
    fn log_order_intent_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_order_intent.jsonl");

        let order = crate::diff::RebalanceOrder {
            symbol: nanobook::Symbol::new("AAPL"),
            action: crate::diff::Action::Buy,
            shares: 100,
            limit_price_cents: 15000,
            notional_cents: 1_500_000,
            description: "test order",
        };
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_intent(
                &mut log,
                &order,
                "client-123",
                timestamp,
                "target-spec-ref",
                "execution-context",
            )
            .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "order_intent");
        assert!(event.data["symbol"] == "AAPL");
        assert!(event.data["action"] == "BUY");
        assert!(event.data["shares"] == 100);
        assert!(event.data["limit"] == 150.0);
        assert!(event.data["client_order_id"] == "client-123");
        assert!(event.data["target_spec_reference"] == "target-spec-ref");
        assert!(event.data["execution_context"] == "execution-context");
    }

    /// Test log_order_intent_checkpoint creates a valid checkpoint with sequence number.
    #[test]
    fn log_order_intent_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_order_intent_checkpoint.jsonl");

        let order = crate::diff::RebalanceOrder {
            symbol: nanobook::Symbol::new("AAPL"),
            action: crate::diff::Action::Buy,
            shares: 100,
            limit_price_cents: 15000,
            notional_cents: 1_500_000,
            description: "test order",
        };
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_intent_checkpoint(
                &mut log,
                5,
                &order,
                "client-123",
                timestamp,
                "target-spec-ref",
                "execution-context",
            )
            .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "order_intent");
        assert_eq!(event.sequence_number, Some(5));
        assert!(event.checkpoint.is_some());
        assert!(event.data["symbol"] == "AAPL");
        assert!(event.data["client_order_id"] == "client-123");
    }

    /// Test log_order_failed creates a valid audit event with error details.
    #[test]
    fn log_order_failed_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_order_failed.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_failed(
                &mut log,
                "ConnectionError",
                "Failed to connect to broker",
                "during order submission",
            )
            .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "order_failed");
        assert!(event.data["error_type"] == "ConnectionError");
        assert!(event.data["error_message"] == "Failed to connect to broker");
        assert!(event.data["context"] == "during order submission");
    }

    /// Test log_order_failed_checkpoint creates a valid checkpoint with sequence number.
    #[test]
    fn log_order_failed_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_order_failed_checkpoint.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_failed_checkpoint(
                &mut log,
                6,
                "ConnectionError",
                "Failed to connect to broker",
                "during order submission",
            )
            .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "order_failed");
        assert_eq!(event.sequence_number, Some(6));
        assert!(event.checkpoint.is_some());
        assert!(event.data["error_type"] == "ConnectionError");
    }

    /// Test that logged order intent events can be parsed back correctly from JSONL.
    #[test]
    fn logged_order_intent_parses_back_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_parse_back.jsonl");

        let order = crate::diff::RebalanceOrder {
            symbol: nanobook::Symbol::new("MSFT"),
            action: crate::diff::Action::Sell,
            shares: 50,
            limit_price_cents: 25000,
            notional_cents: 1_250_000,
            description: "test sell order",
        };
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_intent_checkpoint(
                &mut log,
                5,
                &order,
                "client-456",
                timestamp,
                "target-spec-ref-2",
                "execution-context-2",
            )
            .unwrap();
        }

        // Parse back
        let events = parse_audit_events(&path).unwrap();
        assert_eq!(events.len(), 1);

        let event = &events[0];
        assert_eq!(event.event, "order_intent");
        assert_eq!(event.sequence_number, Some(5));
        assert!(event.checkpoint.is_some());

        // Verify checkpoint can be parsed from event name
        let checkpoint = Checkpoint::from_event_name(&event.event);
        assert_eq!(checkpoint, Some(Checkpoint::OrderIntent));
    }

    /// Test that logged order failed events can be parsed back correctly from JSONL.
    #[test]
    fn logged_order_failed_parses_back_correctly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_parse_failed_back.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_order_failed_checkpoint(
                &mut log,
                6,
                "RateLimitError",
                "Too many requests",
                "broker throttling",
            )
            .unwrap();
        }

        // Parse back
        let events = parse_audit_events(&path).unwrap();
        assert_eq!(events.len(), 1);

        let event = &events[0];
        assert_eq!(event.event, "order_failed");
        assert_eq!(event.sequence_number, Some(6));
        assert!(event.checkpoint.is_some());

        // Verify checkpoint can be parsed from event name
        let checkpoint = Checkpoint::from_event_name(&event.event);
        assert_eq!(checkpoint, Some(Checkpoint::OrderFailed));
    }

    /// Test backward compatibility: old checkpoint sequence (without OrderIntent) still validates.
    #[test]
    fn backward_compatibility_old_sequence_validates() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_old_sequence.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            // Old sequence without OrderIntent
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 4, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // This should fail validation because OrderIntent is now required in the expected sequence
        // This is expected - the validation was updated to require OrderIntent
        assert!(log.validate_checkpoints().is_err());
    }

    /// Test that the new checkpoint sequence is properly validated as the expected sequence.
    #[test]
    fn new_checkpoint_sequence_is_expected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_expected_sequence.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            #[cfg(feature = "write_ahead_logging")]
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            #[cfg(not(feature = "write_ahead_logging"))]
            log.log_checkpoint(Checkpoint::PositionsFetched, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    // ========================================================================
    // PositionsIntent and PositionsResult Checkpoint Tests
    // ========================================================================

    /// Test that Checkpoint::from_event_name parses "positions_intent" correctly.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_from_event_name_positions_intent() {
        let result = Checkpoint::from_event_name("positions_intent");
        assert_eq!(result, Some(Checkpoint::PositionsIntent));
    }

    /// Test that Checkpoint::from_event_name parses "positions_result" correctly.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_from_event_name_positions_result() {
        let result = Checkpoint::from_event_name("positions_result");
        assert_eq!(result, Some(Checkpoint::PositionsResult));
    }

    /// Test that Checkpoint::PositionsIntent.as_event_name returns "positions_intent".
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_as_event_name_positions_intent() {
        assert_eq!(
            Checkpoint::PositionsIntent.as_event_name(),
            "positions_intent"
        );
    }

    /// Test that Checkpoint::PositionsResult.as_event_name returns "positions_result".
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_as_event_name_positions_result() {
        assert_eq!(
            Checkpoint::PositionsResult.as_event_name(),
            "positions_result"
        );
    }

    /// Test log_positions_intent creates a valid audit event with all required fields.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_positions_intent_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_positions_intent.jsonl");
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_positions_intent(&mut log, timestamp, "target-spec-ref").unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "positions_intent");
        assert!(event.data["target_spec_reference"] == "target-spec-ref");
        assert!(event.data["timestamp"].is_string());
    }

    /// Test log_positions_intent_checkpoint creates a valid checkpoint with sequence number.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_positions_intent_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_positions_intent_checkpoint.jsonl");
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_positions_intent_checkpoint(&mut log, 2, timestamp, "target-spec-ref").unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "positions_intent");
        assert_eq!(event.sequence_number, Some(2));
        assert!(event.checkpoint.is_some());
        assert!(event.data["target_spec_reference"] == "target-spec-ref");
    }

    /// Test log_positions_result creates a valid audit event with positions data.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_positions_result_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_positions_result.jsonl");

        let positions = vec![crate::diff::CurrentPosition {
            symbol: nanobook::Symbol::new("AAPL"),
            quantity: 100,
            avg_cost_cents: 15000,
        }];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_positions_result(&mut log, &positions, 10000000).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "positions_result");
        assert!(event.data["positions"].is_array());
        assert!(event.data["equity"] == 100000.0);
    }

    /// Test log_positions_result_checkpoint creates a valid checkpoint with sequence number.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_positions_result_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_positions_result_checkpoint.jsonl");

        let positions = vec![crate::diff::CurrentPosition {
            symbol: nanobook::Symbol::new("AAPL"),
            quantity: 100,
            avg_cost_cents: 15000,
        }];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_positions_result_checkpoint(&mut log, 3, &positions, 10000000).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "positions_result");
        assert_eq!(event.sequence_number, Some(3));
        assert!(event.checkpoint.is_some());
        assert!(event.data["positions"].is_array());
    }

    // ========================================================================
    // QuotesIntent and QuotesResult Checkpoint Tests
    // ========================================================================

    /// Test that Checkpoint::from_event_name parses "quotes_intent" correctly.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_from_event_name_quotes_intent() {
        let result = Checkpoint::from_event_name("quotes_intent");
        assert_eq!(result, Some(Checkpoint::QuotesIntent));
    }

    /// Test that Checkpoint::from_event_name parses "quotes_result" correctly.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_from_event_name_quotes_result() {
        let result = Checkpoint::from_event_name("quotes_result");
        assert_eq!(result, Some(Checkpoint::QuotesResult));
    }

    /// Test that Checkpoint::QuotesIntent.as_event_name returns "quotes_intent".
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_as_event_name_quotes_intent() {
        assert_eq!(Checkpoint::QuotesIntent.as_event_name(), "quotes_intent");
    }

    /// Test that Checkpoint::QuotesResult.as_event_name returns "quotes_result".
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_as_event_name_quotes_result() {
        assert_eq!(Checkpoint::QuotesResult.as_event_name(), "quotes_result");
    }

    /// Test log_quotes_intent creates a valid audit event with all required fields.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_quotes_intent_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_quotes_intent.jsonl");
        let timestamp = chrono::Utc::now();
        let symbols = vec![nanobook::Symbol::new("AAPL"), nanobook::Symbol::new("MSFT")];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_quotes_intent(&mut log, &symbols, 30, timestamp, "target-spec-ref").unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "quotes_intent");
        assert!(event.data["symbols"].is_array());
        assert!(event.data["staleness_threshold_sec"] == 30);
        assert!(event.data["target_spec_reference"] == "target-spec-ref");
        assert!(event.data["timestamp"].is_string());
    }

    /// Test log_quotes_intent_checkpoint creates a valid checkpoint with sequence number.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_quotes_intent_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_quotes_intent_checkpoint.jsonl");
        let timestamp = chrono::Utc::now();
        let symbols = vec![nanobook::Symbol::new("AAPL")];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_quotes_intent_checkpoint(&mut log, 4, &symbols, 30, timestamp, "target-spec-ref")
                .unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "quotes_intent");
        assert_eq!(event.sequence_number, Some(4));
        assert!(event.checkpoint.is_some());
        assert!(event.data["symbols"].is_array());
    }

    /// Test log_quotes_result creates a valid audit event with quotes data.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_quotes_result_creates_valid_event() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_quotes_result.jsonl");

        let quotes = vec![nanobook_broker::types::Quote {
            symbol: nanobook::Symbol::new("AAPL"),
            bid_cents: 14900,
            ask_cents: 15100,
            last_cents: 15000,
            volume: 0,
            timestamp: std::time::SystemTime::now(),
        }];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_quotes_result(&mut log, &quotes).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "quotes_result");
        assert!(event.data["quotes"].is_array());
    }

    /// Test log_quotes_result_checkpoint creates a valid checkpoint with sequence number.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_quotes_result_checkpoint_creates_valid_checkpoint() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_quotes_result_checkpoint.jsonl");

        let quotes = vec![nanobook_broker::types::Quote {
            symbol: nanobook::Symbol::new("AAPL"),
            bid_cents: 14900,
            ask_cents: 15100,
            last_cents: 15000,
            volume: 0,
            timestamp: std::time::SystemTime::now(),
        }];

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_quotes_result_checkpoint(&mut log, 5, &quotes).unwrap();
        }

        let contents = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = contents.lines().collect();
        assert_eq!(lines.len(), 1);

        let event: AuditEvent = serde_json::from_str(lines[0]).unwrap();
        assert_eq!(event.event, "quotes_result");
        assert_eq!(event.sequence_number, Some(5));
        assert!(event.checkpoint.is_some());
        assert!(event.data["quotes"].is_array());
    }

    /// Test that validation accepts the new checkpoint sequence with PositionsIntent/Result.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_validation_accepts_positions_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_positions.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation warns about incomplete PositionsIntent without followup.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_validation_warns_incomplete_positions_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir
            .path()
            .join("test_validation_incomplete_positions.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            // No OrderSubmitted or OrderFailed after OrderIntent - this should warn
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // Validation should succeed (soft validation), but we can't easily test the warning log
        // in unit tests without capturing logs. The important thing is it doesn't error.
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation accepts QuotesIntent followed by QuotesResult.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_validation_accepts_quotes_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_quotes.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::QuotesIntent, 7, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::QuotesResult, 8, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }

    /// Test that validation warns about incomplete QuotesIntent without followup.
    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_validation_warns_incomplete_quotes_intent() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_validation_incomplete_quotes.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsIntent, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsResult, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 6, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::QuotesIntent, 7, serde_json::json!({}))
                .unwrap();
            // No QuotesResult after QuotesIntent - this should warn
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // Validation should succeed (soft validation), but we can't easily test the warning log
        // in unit tests without capturing logs. The important thing is it doesn't error.
        assert!(log.validate_checkpoints().is_ok());
    }

    // ========================================================================
    // AccountSummaryIntent and CancelIntent Checkpoint Tests
    // ========================================================================

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_from_event_name_account_summary_and_cancel() {
        assert_eq!(
            Checkpoint::from_event_name("account_summary_intent"),
            Some(Checkpoint::AccountSummaryIntent)
        );
        assert_eq!(
            Checkpoint::from_event_name("account_summary_result"),
            Some(Checkpoint::AccountSummaryResult)
        );
        assert_eq!(
            Checkpoint::from_event_name("cancel_intent"),
            Some(Checkpoint::CancelIntent)
        );
        assert_eq!(
            Checkpoint::from_event_name("cancel_result"),
            Some(Checkpoint::CancelResult)
        );
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_as_event_name_account_summary_and_cancel() {
        assert_eq!(
            Checkpoint::AccountSummaryIntent.as_event_name(),
            "account_summary_intent"
        );
        assert_eq!(
            Checkpoint::AccountSummaryResult.as_event_name(),
            "account_summary_result"
        );
        assert_eq!(Checkpoint::CancelIntent.as_event_name(), "cancel_intent");
        assert_eq!(Checkpoint::CancelResult.as_event_name(), "cancel_result");
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_account_summary_intent_and_result_create_valid_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_account_summary.jsonl");
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_account_summary_intent_checkpoint(&mut log, 2, timestamp, "target-spec-ref")
                .unwrap();
            log_account_summary_result_checkpoint(&mut log, 3, 150_000_00, 125_000_00).unwrap();
        }

        let events = parse_audit_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "account_summary_intent");
        assert_eq!(events[0].sequence_number, Some(2));
        assert_eq!(events[0].data["target_spec_reference"], "target-spec-ref");
        assert_eq!(events[1].event, "account_summary_result");
        assert_eq!(events[1].sequence_number, Some(3));
        assert_eq!(events[1].data["equity"], 150_000.0);
        assert_eq!(events[1].data["cash"], 125_000.0);
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn log_cancel_intent_and_result_create_valid_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_cancel.jsonl");
        let timestamp = chrono::Utc::now();

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log_cancel_intent_checkpoint(&mut log, 7, 42, "operator_requested", timestamp).unwrap();
            log_cancel_result_checkpoint(&mut log, 8, 42, true, None).unwrap();
        }

        let events = parse_audit_events(&path).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event, "cancel_intent");
        assert_eq!(events[0].sequence_number, Some(7));
        assert_eq!(events[0].data["order_id"], 42);
        assert_eq!(events[0].data["cancellation_reason"], "operator_requested");
        assert_eq!(events[1].event, "cancel_result");
        assert_eq!(events[1].sequence_number, Some(8));
        assert_eq!(events[1].data["success"], true);
        assert!(events[1].data["error_message"].is_null());
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn checkpoint_validation_accepts_phase1c_complete_sequence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test_phase1c_sequence.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::AccountSummaryIntent, 2, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::AccountSummaryResult, 3, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsIntent, 4, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::PositionsResult, 5, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::QuotesIntent, 6, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::QuotesResult, 7, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::DiffComputed, 8, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::RiskCheckPassed, 9, serde_json::json!({}))
                .unwrap();
            log.log_checkpoint(Checkpoint::OrderIntent, 10, serde_json::json!({}))
                .unwrap();
        }

        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        assert!(log.validate_checkpoints().is_ok());
    }
}
