//! JSONL audit trail logging.
//!
//! Each rebalancer run appends events to an audit.jsonl file,
//! one JSON object per line (following nanobook's persistence pattern).

use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Serialize;

use crate::error::Result;

/// An audit event written to the JSONL trail.
#[derive(Debug, Clone, Serialize)]
pub struct AuditEvent {
    pub event: &'static str,
    pub ts: DateTime<Utc>,
    #[serde(flatten)]
    pub data: serde_json::Value,
}

/// Append-only audit logger.
pub struct AuditLog {
    writer: BufWriter<std::fs::File>,
}

impl AuditLog {
    /// Open (or create) the audit log file for appending.
    pub fn open(path: &Path) -> Result<Self> {
        // Ensure parent directory exists
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let file = OpenOptions::new().create(true).append(true).open(path)?;

        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    /// Log an event with arbitrary JSON data.
    pub fn log(&mut self, event: &'static str, data: serde_json::Value) -> Result<()> {
        let entry = AuditEvent {
            event,
            ts: Utc::now(),
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
            let mut log = AuditLog::open(&path).unwrap();
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

        let mut log = AuditLog::open(&path).unwrap();
        log.log_simple("test").unwrap();

        assert!(path.exists());
    }
}
