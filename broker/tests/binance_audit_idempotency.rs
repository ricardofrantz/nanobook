//! Integration tests for Binance audit log idempotency.

use nanobook::Symbol;
use tempfile::TempDir;

use nanobook_broker::binance::{BinanceBroker, log_idempotency_rejection, log_order_submitted};
use nanobook_broker::types::{BrokerOrder, BrokerOrderType, BrokerSide, OrderId};

#[test]
fn test_audit_log_order_submitted() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    // Log an order submission directly
    log_order_submitted(&log_path, OrderId(100), Symbol::new("BTC"), 1, "test-cid");

    // Verify the audit log was written
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

    // Log an idempotency rejection directly
    log_idempotency_rejection(
        &log_path,
        Symbol::new("BTC"),
        1,
        "test-cid",
        "duplicate sequence",
    );

    // Verify the audit log was written
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("idempotency_rejection"));
    assert!(content.contains("BTC"));
    assert!(content.contains("1"));
    assert!(content.contains("test-cid"));
    assert!(content.contains("duplicate sequence"));
}

#[test]
fn test_check_audit_log_for_sequence() {
    use nanobook_broker::binance::{check_audit_log_for_sequence, log_order_submitted};
    use nanobook_broker::types::OrderId;

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
}

#[test]
fn test_audit_log_not_found_returns_false() {
    use nanobook_broker::binance::check_audit_log_for_sequence;

    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("nonexistent.log");

    // Non-existent file should return false, not error
    assert!(!check_audit_log_for_sequence(&log_path, 1).unwrap());
}

#[test]
fn test_submit_with_duplicate_sequence_rejects() {
    use nanobook_broker::binance::log_order_submitted;
    use nanobook_broker::error::BrokerError;
    use nanobook_broker::types::OrderId;

    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    // Pre-populate the audit log with a sequence
    log_order_submitted(&log_path, OrderId(100), Symbol::new("BTC"), 1, "test-cid-1");

    let broker =
        BinanceBroker::new("test_key", "test_secret", true).with_audit_log_path(log_path.clone());

    // Create a test order
    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    // Try to submit with the same sequence number
    let result = broker.submit_order_with_sequence(&order, Some(1));

    // Should be rejected due to duplicate sequence
    assert!(matches!(result, Err(BrokerError::DuplicateOrder { .. })));

    // Verify the rejection was logged
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("idempotency_rejection"));
}
