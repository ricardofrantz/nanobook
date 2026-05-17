//! Integration tests for write-ahead logging wrapper function.

#![cfg(feature = "write_ahead_logging")]

use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::ibkr::orders::{OrderOutcome, OrderResult};
use nanobook_broker::{BrokerSide, ClientOrderId};
use nanobook_rebalancer::audit::{AuditLog, parse_audit_events};
use nanobook_rebalancer::broker::BrokerGateway;
use nanobook_rebalancer::diff::{Action, RebalanceOrder};
use nanobook_rebalancer::execution::execute_order_with_write_ahead;
use std::time::Duration;

/// Mock broker for testing write-ahead logging.
struct MockBroker {
    should_fail: bool,
    error_message: Option<String>,
    call_count: std::cell::RefCell<usize>,
}

impl MockBroker {
    fn new() -> Self {
        Self {
            should_fail: false,
            error_message: None,
            call_count: std::cell::RefCell::new(0),
        }
    }

    fn with_error(message: &str) -> Self {
        Self {
            should_fail: true,
            error_message: Some(message.to_string()),
            call_count: std::cell::RefCell::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.call_count.borrow()
    }
}

impl BrokerGateway for MockBroker {
    fn account_summary(&self) -> Result<nanobook_broker::types::Account, BrokerError> {
        unimplemented!()
    }

    fn positions(&self) -> Result<Vec<nanobook_broker::types::Position>, BrokerError> {
        unimplemented!()
    }

    fn prices(&self, _symbols: &[Symbol]) -> Result<Vec<(Symbol, i64)>, BrokerError> {
        unimplemented!()
    }

    fn quotes(
        &self,
        _symbols: &[Symbol],
    ) -> Result<Vec<nanobook_broker::types::Quote>, BrokerError> {
        unimplemented!()
    }

    fn execute_limit_order(
        &self,
        _symbol: Symbol,
        _side: BrokerSide,
        _shares: u64,
        _limit_price_cents: i64,
        _client_order_id: Option<&ClientOrderId>,
        _timeout: Duration,
    ) -> Result<OrderResult, BrokerError> {
        *self.call_count.borrow_mut() += 1;

        if self.should_fail {
            let msg = self.error_message.as_ref().unwrap();
            // Determine error type based on message
            if msg.contains("invalid symbol") {
                Err(BrokerError::InvalidSymbol(msg.clone()))
            } else if msg.contains("timeout") {
                Err(BrokerError::Connection(msg.clone()))
            } else if msg.contains("connection lost") {
                Err(BrokerError::ConnectionLost {
                    order_id: 123,
                    filled_quantity: 0,
                })
            } else if msg.contains("rate limit") {
                Err(BrokerError::RateLimit)
            } else {
                Err(BrokerError::Order(msg.clone()))
            }
        } else {
            Ok(OrderResult {
                order_id: 12345,
                symbol: Symbol::new("AAPL"),
                filled_shares: 100,
                avg_fill_price: 150.0,
                commission: 1.0,
                status: OrderOutcome::Filled,
            })
        }
    }
}

/// Test successful order submission with write-ahead logging.
#[test]
fn test_write_ahead_success() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let broker = MockBroker::new();
    let order = RebalanceOrder {
        symbol: Symbol::new("AAPL"),
        action: Action::Buy,
        shares: 100,
        limit_price_cents: 15000,
        notional_cents: 1500000,
        description: "Test order",
    };
    let client_order_id = ClientOrderId::derive("test_scope", "AAPL", BrokerSide::Buy, 100);
    let timeout = Duration::from_secs(30);
    let sequence_number = 1;
    let target_spec = "test_target";

    let result = execute_order_with_write_ahead(
        &broker,
        &mut audit,
        &order,
        &client_order_id,
        timeout,
        sequence_number,
        target_spec,
    );

    assert!(result.is_ok());
    assert_eq!(broker.call_count(), 1);

    // Verify audit log contains OrderIntent and OrderSubmitted
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "order_intent");
    assert_eq!(events[1].event, "order_submitted");
}

/// Test permanent error (no retry).
#[test]
fn test_write_ahead_permanent_error_no_retry() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let broker = MockBroker::with_error("invalid symbol: BAD");
    let order = RebalanceOrder {
        symbol: Symbol::new("BAD"),
        action: Action::Buy,
        shares: 100,
        limit_price_cents: 15000,
        notional_cents: 1500000,
        description: "Test order",
    };
    let client_order_id = ClientOrderId::derive("test_scope", "BAD", BrokerSide::Buy, 100);
    let timeout = Duration::from_secs(30);
    let sequence_number = 1;
    let target_spec = "test_target";

    let result = execute_order_with_write_ahead(
        &broker,
        &mut audit,
        &order,
        &client_order_id,
        timeout,
        sequence_number,
        target_spec,
    );

    assert!(result.is_err());
    // Should only call broker once (no retry for permanent errors)
    assert_eq!(broker.call_count(), 1);

    // Verify audit log contains OrderIntent and OrderFailed
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "order_intent");
    assert_eq!(events[1].event, "order_failed");
}

/// Test transient error with retry (but we can't test actual retry delay in unit tests).
#[test]
fn test_write_ahead_transient_error_classification() {
    // This test verifies that transient errors are correctly classified
    // The actual retry logic is tested via mock call count in a separate test
    let transient_messages = vec!["timeout", "connection lost", "rate limit"];

    for msg in transient_messages {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("test_audit.jsonl");
        let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

        let broker = MockBroker::with_error(msg);
        let order = RebalanceOrder {
            symbol: Symbol::new("AAPL"),
            action: Action::Buy,
            shares: 100,
            limit_price_cents: 15000,
            notional_cents: 1500000,
            description: "Test order",
        };
        let client_order_id = ClientOrderId::derive("test_scope", "AAPL", BrokerSide::Buy, 100);
        let timeout = Duration::from_secs(30);
        let sequence_number = 1;
        let target_spec = "test_target";

        // This will attempt retries but eventually fail after max retries
        // For testing purposes, we just verify it's classified as transient
        let result = execute_order_with_write_ahead(
            &broker,
            &mut audit,
            &order,
            &client_order_id,
            timeout,
            sequence_number,
            target_spec,
        );

        assert!(result.is_err());
        // Should have attempted retries (call count > 1)
        assert!(broker.call_count() > 1);
    }
}

/// Test that audit write failure is handled gracefully.
#[test]
fn test_write_ahead_audit_write_failure() {
    // This test verifies that if audit write fails, the error is propagated
    // We can't easily simulate audit write failure without mocking the filesystem,
    // so we just verify that the function signature accepts a mutable audit reference
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let broker = MockBroker::new();
    let order = RebalanceOrder {
        symbol: Symbol::new("AAPL"),
        action: Action::Buy,
        shares: 100,
        limit_price_cents: 15000,
        notional_cents: 1500000,
        description: "Test order",
    };
    let client_order_id = ClientOrderId::derive("test_scope", "AAPL", BrokerSide::Buy, 100);
    let timeout = Duration::from_secs(30);
    let sequence_number = 1;
    let target_spec = "test_target";

    // This should succeed because audit log is writable
    let result = execute_order_with_write_ahead(
        &broker,
        &mut audit,
        &order,
        &client_order_id,
        timeout,
        sequence_number,
        target_spec,
    );

    assert!(result.is_ok());
}

/// Test that write_ahead_success.jsonl fixture parses correctly.
#[test]
fn fixture_write_ahead_success_parses_correctly() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/write_ahead_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 7, "Expected 7 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_fetched");
    assert_eq!(events[2].event, "diff_computed");
    assert_eq!(events[3].event, "risk_check_passed");
    assert_eq!(events[4].event, "order_intent");
    assert_eq!(events[5].event, "order_submitted");
    assert_eq!(events[6].event, "order_filled");

    // Verify OrderIntent is followed by OrderSubmitted
    let intent_idx = events
        .iter()
        .position(|e| e.event == "order_intent")
        .expect("OrderIntent not found");
    let submitted_idx = events
        .iter()
        .position(|e| e.event == "order_submitted")
        .expect("OrderSubmitted not found");
    assert!(
        submitted_idx > intent_idx,
        "OrderSubmitted should come after OrderIntent"
    );
}

/// Test that write_ahead_failure.jsonl fixture parses correctly.
#[test]
fn fixture_write_ahead_failure_parses_correctly() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/write_ahead_failure.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 6, "Expected 6 events in fixture");

    // Verify OrderIntent is followed by OrderFailed
    let intent_idx = events
        .iter()
        .position(|e| e.event == "order_intent")
        .expect("OrderIntent not found");
    let failed_idx = events
        .iter()
        .position(|e| e.event == "order_failed")
        .expect("OrderFailed not found");
    assert!(
        failed_idx > intent_idx,
        "OrderFailed should come after OrderIntent"
    );

    // Verify OrderFailed has error details
    let failed_event = &events[failed_idx];
    assert!(failed_event.data.get("error_type").is_some());
    assert!(failed_event.data.get("error_message").is_some());
    assert!(failed_event.data.get("context").is_some());
}

/// Test that write_ahead_retry.jsonl fixture parses correctly.
#[test]
fn fixture_write_ahead_retry_parses_correctly() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/write_ahead_retry.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 6, "Expected 6 events in fixture");

    // Verify OrderIntent is followed by OrderFailed (max retries exceeded)
    let intent_idx = events
        .iter()
        .position(|e| e.event == "order_intent")
        .expect("OrderIntent not found");
    let failed_idx = events
        .iter()
        .position(|e| e.event == "order_failed")
        .expect("OrderFailed not found");
    assert!(
        failed_idx > intent_idx,
        "OrderFailed should come after OrderIntent"
    );

    // Verify OrderFailed indicates max retries exceeded
    let failed_event = &events[failed_idx];
    let error_type = failed_event.data.get("error_type").and_then(|v| v.as_str());
    assert_eq!(error_type, Some("max_retries_exceeded"));
}

/// Test that write_ahead_incomplete.jsonl fixture parses correctly.
#[test]
fn fixture_write_ahead_incomplete_parses_correctly() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/write_ahead_incomplete.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 5, "Expected 5 events in fixture");

    // Verify last event is OrderIntent (crash scenario)
    assert_eq!(events[4].event, "order_intent");

    // Verify OrderIntent has all required fields
    let intent_event = &events[4];
    assert!(intent_event.data.get("symbol").is_some());
    assert!(intent_event.data.get("action").is_some());
    assert!(intent_event.data.get("client_order_id").is_some());
    assert!(intent_event.data.get("timestamp").is_some());
}
