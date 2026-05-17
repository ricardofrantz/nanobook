//! Integration tests for PositionsIntent and QuotesIntent write-ahead logging.

#![cfg(feature = "write_ahead_logging")]

use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::{BrokerSide, ClientOrderId};
use nanobook_rebalancer::audit::{
    AuditLog, Checkpoint, parse_audit_events, validate_checkpoints_from_parsed,
};
use nanobook_rebalancer::audit::{
    log_positions_intent_checkpoint, log_positions_result_checkpoint, log_quotes_intent_checkpoint,
    log_quotes_result_checkpoint,
};
use nanobook_rebalancer::broker::BrokerGateway;
use nanobook_rebalancer::diff::CurrentPosition;
use std::path::PathBuf;
use std::time::Duration;

/// Mock broker for testing positions/quotes write-ahead logging.
struct MockBroker {
    should_fail_positions: bool,
    should_fail_quotes: bool,
    positions_call_count: std::cell::RefCell<usize>,
    quotes_call_count: std::cell::RefCell<usize>,
}

impl MockBroker {
    fn new() -> Self {
        Self {
            should_fail_positions: false,
            should_fail_quotes: false,
            positions_call_count: std::cell::RefCell::new(0),
            quotes_call_count: std::cell::RefCell::new(0),
        }
    }

    fn with_positions_error() -> Self {
        Self {
            should_fail_positions: true,
            should_fail_quotes: false,
            positions_call_count: std::cell::RefCell::new(0),
            quotes_call_count: std::cell::RefCell::new(0),
        }
    }

    fn with_quotes_error() -> Self {
        Self {
            should_fail_positions: false,
            should_fail_quotes: true,
            positions_call_count: std::cell::RefCell::new(0),
            quotes_call_count: std::cell::RefCell::new(0),
        }
    }

    fn positions_call_count(&self) -> usize {
        *self.positions_call_count.borrow()
    }

    fn quotes_call_count(&self) -> usize {
        *self.quotes_call_count.borrow()
    }
}

impl BrokerGateway for MockBroker {
    fn account_summary(&self) -> Result<nanobook_broker::types::Account, BrokerError> {
        unimplemented!()
    }

    fn positions(&self) -> Result<Vec<nanobook_broker::types::Position>, BrokerError> {
        *self.positions_call_count.borrow_mut() += 1;

        if self.should_fail_positions {
            Err(BrokerError::Connection(
                "Failed to fetch positions".to_string(),
            ))
        } else {
            Ok(vec![nanobook_broker::types::Position {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 15000,
                market_value_cents: 1500000,
                unrealized_pnl_cents: 0,
            }])
        }
    }

    fn prices(&self, _symbols: &[Symbol]) -> Result<Vec<(Symbol, i64)>, BrokerError> {
        unimplemented!()
    }

    fn quotes(
        &self,
        symbols: &[Symbol],
    ) -> Result<Vec<nanobook_broker::types::Quote>, BrokerError> {
        *self.quotes_call_count.borrow_mut() += 1;

        if self.should_fail_quotes {
            Err(BrokerError::Connection(
                "Failed to fetch quotes".to_string(),
            ))
        } else {
            Ok(symbols
                .iter()
                .map(|s| nanobook_broker::types::Quote {
                    symbol: s.clone(),
                    bid_cents: 14900,
                    ask_cents: 15100,
                    last_cents: 15000,
                    volume: 0,
                    timestamp: std::time::SystemTime::now(),
                })
                .collect())
        }
    }

    fn execute_limit_order(
        &self,
        _symbol: Symbol,
        _side: BrokerSide,
        _shares: u64,
        _limit_price_cents: i64,
        _client_order_id: Option<&ClientOrderId>,
        _timeout: Duration,
    ) -> Result<nanobook_broker::ibkr::orders::OrderResult, BrokerError> {
        unimplemented!()
    }
}

/// Test successful positions fetch with write-ahead logging.
#[test]
fn test_positions_fetch_with_write_ahead() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let sequence_number = 2;
    let target_spec = "test_target";

    // Log positions intent
    log_positions_intent_checkpoint(&mut audit, sequence_number, timestamp, target_spec).unwrap();

    // Fetch positions
    let broker = MockBroker::new();
    let positions_result = broker.positions().unwrap();

    // Convert to CurrentPosition
    let current_positions: Vec<CurrentPosition> = positions_result
        .iter()
        .map(|p| CurrentPosition {
            symbol: p.symbol.clone(),
            quantity: p.quantity,
            avg_cost_cents: p.avg_cost_cents,
        })
        .collect();

    // Log positions result
    let equity_cents = 15000_00;
    log_positions_result_checkpoint(
        &mut audit,
        sequence_number + 1,
        &current_positions,
        equity_cents,
    )
    .unwrap();

    // Verify audit log contains positions_intent and positions_result
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "positions_intent");
    assert_eq!(events[1].event, "positions_result");
    assert_eq!(broker.positions_call_count(), 1);
}

/// Test successful quotes fetch with write-ahead logging.
#[test]
fn test_quotes_fetch_with_write_ahead() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let sequence_number = 3;
    let target_spec = "test_target";
    let symbols = vec![Symbol::new("AAPL"), Symbol::new("MSFT")];
    let staleness_threshold_sec = 30;

    // Log quotes intent
    log_quotes_intent_checkpoint(
        &mut audit,
        sequence_number,
        &symbols,
        staleness_threshold_sec,
        timestamp,
        target_spec,
    )
    .unwrap();

    // Fetch quotes
    let broker = MockBroker::new();
    let quotes_result = broker.quotes(&symbols).unwrap();

    // Log quotes result
    log_quotes_result_checkpoint(&mut audit, sequence_number + 1, &quotes_result).unwrap();

    // Verify audit log contains quotes_intent and quotes_result
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "quotes_intent");
    assert_eq!(events[1].event, "quotes_result");
    assert_eq!(broker.quotes_call_count(), 1);
}

/// Test positions fetch failure with write-ahead logging.
#[test]
fn test_positions_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let sequence_number = 2;
    let target_spec = "test_target";

    // Log positions intent
    log_positions_intent_checkpoint(&mut audit, sequence_number, timestamp, target_spec).unwrap();

    // Attempt to fetch positions (will fail)
    let broker = MockBroker::with_positions_error();
    let positions_result = broker.positions();

    // Verify fetch failed
    assert!(positions_result.is_err());

    // In a real scenario, we would log a failure event here
    // For this test, we just verify the intent was logged
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "positions_intent");
    assert_eq!(broker.positions_call_count(), 1);
}

/// Test quotes fetch failure with write-ahead logging.
#[test]
fn test_quotes_fetch_failure() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let sequence_number = 3;
    let target_spec = "test_target";
    let symbols = vec![Symbol::new("AAPL"), Symbol::new("MSFT")];
    let staleness_threshold_sec = 30;

    // Log quotes intent
    log_quotes_intent_checkpoint(
        &mut audit,
        sequence_number,
        &symbols,
        staleness_threshold_sec,
        timestamp,
        target_spec,
    )
    .unwrap();

    // Attempt to fetch quotes (will fail)
    let broker = MockBroker::with_quotes_error();
    let quotes_result = broker.quotes(&symbols);

    // Verify fetch failed
    assert!(quotes_result.is_err());

    // In a real scenario, we would log a failure event here
    // For this test, we just verify the intent was logged
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "quotes_intent");
    assert_eq!(broker.quotes_call_count(), 1);
}

/// Test audit log validity: positions_intent to positions_result ratio should be 1:1.
#[test]
fn test_positions_intent_to_result_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let target_spec = "test_target";

    // Simulate successful fetch
    log_positions_intent_checkpoint(&mut audit, 2, timestamp, target_spec).unwrap();

    let broker = MockBroker::new();
    let positions_result = broker.positions().unwrap();

    let current_positions: Vec<CurrentPosition> = positions_result
        .iter()
        .map(|p| CurrentPosition {
            symbol: p.symbol.clone(),
            quantity: p.quantity,
            avg_cost_cents: p.avg_cost_cents,
        })
        .collect();

    log_positions_result_checkpoint(&mut audit, 3, &current_positions, 15000_00).unwrap();

    // Verify ratio
    let events = parse_audit_events(&audit_path).unwrap();
    let intent_count = events
        .iter()
        .filter(|e| e.event == "positions_intent")
        .count();
    let result_count = events
        .iter()
        .filter(|e| e.event == "positions_result")
        .count();

    assert_eq!(intent_count, 1);
    assert_eq!(result_count, 1);
    assert_eq!(
        intent_count, result_count,
        "Intent:Result ratio should be 1:1"
    );
}

/// Test audit log validity: quotes_intent to quotes_result ratio should be 1:1.
#[test]
fn test_quotes_intent_to_result_ratio() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("test_audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();

    let timestamp = chrono::Utc::now();
    let target_spec = "test_target";
    let symbols = vec![Symbol::new("AAPL")];
    let staleness_threshold_sec = 30;

    // Simulate successful fetch
    log_quotes_intent_checkpoint(
        &mut audit,
        3,
        &symbols,
        staleness_threshold_sec,
        timestamp,
        target_spec,
    )
    .unwrap();

    let broker = MockBroker::new();
    let quotes_result = broker.quotes(&symbols).unwrap();

    log_quotes_result_checkpoint(&mut audit, 4, &quotes_result).unwrap();

    // Verify ratio
    let events = parse_audit_events(&audit_path).unwrap();
    let intent_count = events.iter().filter(|e| e.event == "quotes_intent").count();
    let result_count = events.iter().filter(|e| e.event == "quotes_result").count();

    assert_eq!(intent_count, 1);
    assert_eq!(result_count, 1);
    assert_eq!(
        intent_count, result_count,
        "Intent:Result ratio should be 1:1"
    );
}

/// Test recovery from crash at PositionsIntent checkpoint.
#[test]
fn test_positions_intent_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at positions_intent checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::PositionsIntent,
            2,
            serde_json::json!({
                "timestamp": "2024-01-15T10:00:01Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
    }

    // Recover state
    let (state, action) = nanobook_rebalancer::recovery::reconstruct_state(&audit_path).unwrap();

    assert_eq!(
        state.checkpoint,
        nanobook_rebalancer::audit::Checkpoint::PositionsIntent
    );
    assert_eq!(state.sequence_number, 2);
    assert!(!state.run_completed);
    assert_eq!(
        action,
        nanobook_rebalancer::recovery::RecoveryAction::Restart
    );
}

/// Test recovery from crash at QuotesIntent checkpoint.
#[test]
fn test_quotes_intent_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at quotes_intent checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::PositionsFetched,
            2,
            serde_json::json!({
                "positions": [{
                    "symbol": "AAPL",
                    "qty": 100,
                    "avg_cost": 150.0
                }]
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::DiffComputed,
            3,
            serde_json::json!({
                "orders": [{
                    "symbol": "AAPL",
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "description": "test"
                }]
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::QuotesIntent,
            4,
            serde_json::json!({
                "symbols": ["AAPL", "MSFT"],
                "staleness_threshold_sec": 30,
                "timestamp": "2024-01-15T10:00:04Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
    }

    // Recover state
    let (state, action) = nanobook_rebalancer::recovery::reconstruct_state(&audit_path).unwrap();

    assert_eq!(
        state.checkpoint,
        nanobook_rebalancer::audit::Checkpoint::QuotesIntent
    );
    assert_eq!(state.sequence_number, 4);
    assert!(!state.run_completed);
    assert_eq!(
        action,
        nanobook_rebalancer::recovery::RecoveryAction::Restart
    );
}

/// Test recovery from crash at PositionsResult checkpoint.
#[test]
fn test_positions_result_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at positions_result checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::PositionsIntent,
            2,
            serde_json::json!({
                "timestamp": "2024-01-15T10:00:01Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::PositionsResult,
            3,
            serde_json::json!({
                "positions": [{
                    "symbol": "AAPL",
                    "qty": 100,
                    "avg_cost": 150.0
                }],
                "equity": 15000.0
            }),
        )
        .unwrap();
    }

    // Recover state
    let (state, action) = nanobook_rebalancer::recovery::reconstruct_state(&audit_path).unwrap();

    assert_eq!(
        state.checkpoint,
        nanobook_rebalancer::audit::Checkpoint::PositionsResult
    );
    assert_eq!(state.sequence_number, 3);
    assert!(!state.run_completed);
    assert_eq!(
        action,
        nanobook_rebalancer::recovery::RecoveryAction::Restart
    );
}

/// Test recovery from crash at QuotesResult checkpoint.
#[test]
fn test_quotes_result_crash_recovery() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at quotes_result checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::PositionsFetched,
            2,
            serde_json::json!({
                "positions": [{
                    "symbol": "AAPL",
                    "qty": 100,
                    "avg_cost": 150.0
                }]
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::DiffComputed,
            3,
            serde_json::json!({
                "orders": [{
                    "symbol": "AAPL",
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "description": "test"
                }]
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::QuotesIntent,
            4,
            serde_json::json!({
                "symbols": ["AAPL", "MSFT"],
                "staleness_threshold_sec": 30,
                "timestamp": "2024-01-15T10:00:04Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::QuotesResult,
            5,
            serde_json::json!({
                "quotes": [{
                    "symbol": "AAPL",
                    "bid": 149.0,
                    "ask": 151.0,
                    "timestamp": "2024-01-15T10:00:05Z"
                }, {
                    "symbol": "MSFT",
                    "bid": 379.0,
                    "ask": 381.0,
                    "timestamp": "2024-01-15T10:00:05Z"
                }]
            }),
        )
        .unwrap();
    }

    // Recover state
    let (state, action) = nanobook_rebalancer::recovery::reconstruct_state(&audit_path).unwrap();

    assert_eq!(
        state.checkpoint,
        nanobook_rebalancer::audit::Checkpoint::QuotesResult
    );
    assert_eq!(state.sequence_number, 5);
    assert!(!state.run_completed);
    assert_eq!(
        action,
        nanobook_rebalancer::recovery::RecoveryAction::Restart
    );
}

// ============================================================================
// Golden Fixture Tests
// ============================================================================

/// Test that positions_intent_only.jsonl (crash scenario) parses correctly.
#[test]
fn fixture_positions_intent_only_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/positions_intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 2, "Expected 2 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_intent");

    // Verify all have sequence numbers
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number,
            Some((i + 1) as u64),
            "Event {} missing sequence number",
            i
        );
    }

    // Verify PositionsIntent has all required fields
    let intent_event = &events[1];
    assert_eq!(intent_event.event, "positions_intent");
    assert!(intent_event.data.get("timestamp").is_some());
    assert!(intent_event.data.get("target_spec_reference").is_some());
}

/// Test that positions_intent_only.jsonl validates correctly.
/// Note: This fixture represents an incomplete crash scenario, so validation
/// is expected to fail (it doesn't have a complete checkpoint sequence).
#[test]
fn fixture_positions_intent_only_validates_incomplete() {
    let fixture_path = PathBuf::from("tests/fixtures/positions_intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should fail because the sequence is incomplete
    let result = validate_checkpoints_from_parsed(&events);
    assert!(
        result.is_err(),
        "Validation should fail for incomplete sequence"
    );
}

/// Test that positions_intent_success.jsonl parses correctly.
#[test]
fn fixture_positions_intent_success_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/positions_intent_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 3, "Expected 3 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_intent");
    assert_eq!(events[2].event, "positions_result");

    // Verify PositionsIntent is followed by PositionsResult
    let intent_idx = events
        .iter()
        .position(|e| e.event == "positions_intent")
        .expect("PositionsIntent not found");
    let result_idx = events
        .iter()
        .position(|e| e.event == "positions_result")
        .expect("PositionsResult not found");
    assert!(
        result_idx > intent_idx,
        "PositionsResult should come after PositionsIntent"
    );
}

/// Test that positions_intent_success.jsonl validates correctly.
/// Note: This fixture is also incomplete (doesn't have full checkpoint sequence),
/// so validation is expected to fail.
#[test]
fn fixture_positions_intent_success_validates_incomplete() {
    let fixture_path = PathBuf::from("tests/fixtures/positions_intent_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should fail because the sequence is incomplete
    let result = validate_checkpoints_from_parsed(&events);
    assert!(
        result.is_err(),
        "Validation should fail for incomplete sequence"
    );
}

/// Test that quotes_intent_only.jsonl (crash scenario) parses correctly.
#[test]
fn fixture_quotes_intent_only_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/quotes_intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 5, "Expected 5 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_fetched");
    assert_eq!(events[2].event, "diff_computed");
    assert_eq!(events[3].event, "risk_check_passed");
    assert_eq!(events[4].event, "quotes_intent");

    // Verify all have sequence numbers
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number,
            Some((i + 1) as u64),
            "Event {} missing sequence number",
            i
        );
    }

    // Verify QuotesIntent has all required fields
    let intent_event = &events[4];
    assert_eq!(intent_event.event, "quotes_intent");
    assert!(intent_event.data.get("symbols").is_some());
    assert!(intent_event.data.get("staleness_threshold_sec").is_some());
    assert!(intent_event.data.get("timestamp").is_some());
    assert!(intent_event.data.get("target_spec_reference").is_some());
}

/// Test that quotes_intent_only.jsonl validates correctly.
/// Note: This fixture represents an incomplete crash scenario, so validation
/// is expected to fail (it doesn't have a complete checkpoint sequence).
#[test]
fn fixture_quotes_intent_only_validates_incomplete() {
    let fixture_path = PathBuf::from("tests/fixtures/quotes_intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should fail because the sequence is incomplete
    let result = validate_checkpoints_from_parsed(&events);
    assert!(
        result.is_err(),
        "Validation should fail for incomplete sequence"
    );
}

/// Test that quotes_intent_success.jsonl parses correctly.
#[test]
fn fixture_quotes_intent_success_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/quotes_intent_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 6, "Expected 6 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_fetched");
    assert_eq!(events[2].event, "diff_computed");
    assert_eq!(events[3].event, "risk_check_passed");
    assert_eq!(events[4].event, "quotes_intent");
    assert_eq!(events[5].event, "quotes_result");

    // Verify QuotesIntent is followed by QuotesResult
    let intent_idx = events
        .iter()
        .position(|e| e.event == "quotes_intent")
        .expect("QuotesIntent not found");
    let result_idx = events
        .iter()
        .position(|e| e.event == "quotes_result")
        .expect("QuotesResult not found");
    assert!(
        result_idx > intent_idx,
        "QuotesResult should come after QuotesIntent"
    );
}

/// Test that quotes_intent_success.jsonl validates correctly.
/// Note: This fixture is also incomplete (doesn't have full checkpoint sequence),
/// so validation is expected to fail.
#[test]
fn fixture_quotes_intent_success_validates_incomplete() {
    let fixture_path = PathBuf::from("tests/fixtures/quotes_intent_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should fail because the sequence is incomplete
    let result = validate_checkpoints_from_parsed(&events);
    assert!(
        result.is_err(),
        "Validation should fail for incomplete sequence"
    );
}

/// Test that checkpoints can be round-tripped through the audit log for PositionsIntent.
#[test]
fn checkpoint_roundtrip_positions_intent() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            Checkpoint::PositionsIntent,
            2,
            serde_json::json!({
                "timestamp": "2024-01-15T10:00:01Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
    }

    // Parse back
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.event, "positions_intent");
    assert_eq!(event.sequence_number, Some(2));
    assert!(event.checkpoint.is_some());

    // Verify checkpoint can be parsed from event name
    let checkpoint = Checkpoint::from_event_name(&event.event);
    assert_eq!(checkpoint, Some(Checkpoint::PositionsIntent));

    // Verify the checkpoint can be converted back to event name
    if let Some(cp) = checkpoint {
        assert_eq!(cp.as_event_name(), "positions_intent");
    }
}

/// Test that checkpoints can be round-tripped through the audit log for QuotesIntent.
#[test]
fn checkpoint_roundtrip_quotes_intent() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            Checkpoint::QuotesIntent,
            5,
            serde_json::json!({
                "symbols": ["AAPL", "MSFT"],
                "staleness_threshold_sec": 30,
                "timestamp": "2024-01-15T10:00:04Z",
                "target_spec_reference": "target.json"
            }),
        )
        .unwrap();
    }

    // Parse back
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.event, "quotes_intent");
    assert_eq!(event.sequence_number, Some(5));
    assert!(event.checkpoint.is_some());

    // Verify checkpoint can be parsed from event name
    let checkpoint = Checkpoint::from_event_name(&event.event);
    assert_eq!(checkpoint, Some(Checkpoint::QuotesIntent));

    // Verify the checkpoint can be converted back to event name
    if let Some(cp) = checkpoint {
        assert_eq!(cp.as_event_name(), "quotes_intent");
    }
}
