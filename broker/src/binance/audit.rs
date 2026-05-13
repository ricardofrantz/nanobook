//! Audit log integration for Binance idempotency tracking.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use chrono::Utc;
use nanobook::Symbol;
use serde::Serialize;

use crate::types::OrderId;

/// Audit log event types.
#[derive(Debug, Serialize)]
#[serde(tag = "event_type")]
enum AuditEvent {
    #[serde(rename = "order_submitted")]
    OrderSubmitted {
        timestamp: String,
        order_id: u64,
        symbol: String,
        sequence: u64,
        client_order_id: String,
    },
    #[serde(rename = "order_filled")]
    OrderFilled {
        timestamp: String,
        order_id: u64,
        sequence: u64,
    },
    #[serde(rename = "idempotency_rejection")]
    IdempotencyRejection {
        timestamp: String,
        symbol: String,
        sequence: u64,
        client_order_id: String,
        reason: String,
    },
}

/// Log an order submission event to the audit log.
pub fn log_order_submitted(
    path: &Path,
    order_id: OrderId,
    symbol: Symbol,
    sequence: u64,
    client_order_id: &str,
) {
    let event = AuditEvent::OrderSubmitted {
        timestamp: Utc::now().to_rfc3339(),
        order_id: order_id.0,
        symbol: symbol.as_str().to_string(),
        sequence,
        client_order_id: client_order_id.to_string(),
    };

    write_audit_event(path, &event);
}

/// Log an order fill event to the audit log.
pub fn log_order_filled(path: &Path, order_id: OrderId, sequence: u64) {
    let event = AuditEvent::OrderFilled {
        timestamp: Utc::now().to_rfc3339(),
        order_id: order_id.0,
        sequence,
    };

    write_audit_event(path, &event);
}

/// Log an idempotency rejection event to the audit log.
pub fn log_idempotency_rejection(
    path: &Path,
    symbol: Symbol,
    sequence: u64,
    client_order_id: &str,
    reason: &str,
) {
    let event = AuditEvent::IdempotencyRejection {
        timestamp: Utc::now().to_rfc3339(),
        symbol: symbol.as_str().to_string(),
        sequence,
        client_order_id: client_order_id.to_string(),
        reason: reason.to_string(),
    };

    write_audit_event(path, &event);
}

/// Write an audit event to the log file (JSONL format).
fn write_audit_event(path: &Path, event: &AuditEvent) {
    if let Ok(json) = serde_json::to_string(event) {
        if let Ok(mut file) = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            if let Err(e) = writeln!(file, "{}", json) {
                log::warn!("Failed to write audit log entry: {}", e);
            }
        } else {
            log::warn!("Failed to open audit log file for writing: {:?}", path);
        }
    } else {
        log::warn!("Failed to serialize audit event: {:?}", event);
    }
}

/// Check if a sequence number already exists in the audit log.
///
/// Returns true if an OrderSubmitted event with the same sequence number is found,
/// false otherwise. Handles file not found as false (no audit log = no duplicate).
pub fn check_audit_log_for_sequence(path: &Path, sequence: u64) -> Result<bool, std::io::Error> {
    // If file doesn't exist, no duplicate
    if !path.exists() {
        return Ok(false);
    }

    let content = std::fs::read_to_string(path)?;
    for line in content.lines() {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(line) {
            // Check if this is an OrderSubmitted event with matching sequence
            if let Some(event_type) = value.get("event_type").and_then(|v| v.as_str()) {
                if event_type == "order_submitted" {
                    if let Some(seq) = value.get("sequence").and_then(|v| v.as_u64()) {
                        if seq == sequence {
                            return Ok(true);
                        }
                    }
                }
            }
        }
    }

    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_check_audit_log_for_sequence() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.log");

        // Initially, no sequence exists
        assert!(!check_audit_log_for_sequence(&log_path, 1).unwrap());

        // Log an order with sequence 1
        log_order_submitted(&log_path, OrderId(100), Symbol::new("BTC"), 1, "test-cid-1");

        // Now sequence 1 should be found
        assert!(check_audit_log_for_sequence(&log_path, 1).unwrap());

        // Sequence 2 should not be found
        assert!(!check_audit_log_for_sequence(&log_path, 2).unwrap());

        // Log another order with sequence 2
        log_order_submitted(&log_path, OrderId(101), Symbol::new("ETH"), 2, "test-cid-2");

        // Both sequences should be found
        assert!(check_audit_log_for_sequence(&log_path, 1).unwrap());
        assert!(check_audit_log_for_sequence(&log_path, 2).unwrap());
    }

    #[test]
    fn test_audit_log_not_found_returns_false() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("nonexistent.log");

        // Non-existent file should return false, not error
        assert!(!check_audit_log_for_sequence(&log_path, 1).unwrap());
    }

    #[test]
    fn test_audit_log_order_submitted() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.log");

        log_order_submitted(&log_path, OrderId(100), Symbol::new("BTC"), 1, "test-cid");

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("order_submitted"));
        assert!(content.contains("100"));
        assert!(content.contains("BTC"));
        assert!(content.contains("1"));
        assert!(content.contains("test-cid"));
    }

    #[test]
    fn test_audit_log_idempotency_rejection() {
        let temp_dir = TempDir::new().unwrap();
        let log_path = temp_dir.path().join("audit.log");

        log_idempotency_rejection(
            &log_path,
            Symbol::new("BTC"),
            1,
            "test-cid",
            "duplicate sequence",
        );

        let content = std::fs::read_to_string(&log_path).unwrap();
        assert!(content.contains("idempotency_rejection"));
        assert!(content.contains("BTC"));
        assert!(content.contains("1"));
        assert!(content.contains("test-cid"));
        assert!(content.contains("duplicate sequence"));
    }
}
