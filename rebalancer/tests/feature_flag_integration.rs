// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Integration tests for write_ahead_logging feature flag.

use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::ibkr::orders::{OrderOutcome, OrderResult};
use nanobook_broker::{BrokerSide, ClientOrderId};
use nanobook_rebalancer::audit::{parse_audit_events, AuditLog, Checkpoint};
use nanobook_rebalancer::diff::{Action, RebalanceOrder};
use nanobook_rebalancer::execution::execute_order_with_write_ahead;
use nanobook_rebalancer::broker::BrokerGateway;
use nanobook_rebalancer::recovery::{reconstruct_state, RecoveryAction};
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

    fn quotes(&self, _symbols: &[Symbol]) -> Result<Vec<nanobook_broker::types::Quote>, BrokerError> {
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
            Err(BrokerError::Order(msg.clone()))
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

// ============================================================================
// Integration tests for order submission
// ============================================================================

#[test]
fn test_order_submission_without_feature() {
    // When feature is disabled, execute_order_with_write_ahead should call broker directly
    // without logging OrderIntent checkpoint
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

    // When feature is disabled, audit log should NOT contain OrderIntent
    let events = parse_audit_events(&audit_path).unwrap();
    #[cfg(not(feature = "write_ahead_logging"))]
    {
        // Without feature, no OrderIntent checkpoint should be logged
        assert!(!events.iter().any(|e| e.event == "order_intent"));
    }

    #[cfg(feature = "write_ahead_logging")]
    {
        // With feature, OrderIntent should be logged
        assert!(events.iter().any(|e| e.event == "order_intent"));
    }
}

#[test]
fn test_order_submission_with_feature() {
    // When feature is enabled, execute_order_with_write_ahead should log OrderIntent
    // before calling the broker
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

    #[cfg(feature = "write_ahead_logging")]
    {
        // With feature, OrderIntent should be logged before broker call
        let events = parse_audit_events(&audit_path).unwrap();
        assert!(events.iter().any(|e| e.event == "order_intent"));
    }

    #[cfg(not(feature = "write_ahead_logging"))]
    {
        // Without feature, no OrderIntent checkpoint should be logged
        let events = parse_audit_events(&audit_path).unwrap();
        assert!(!events.iter().any(|e| e.event == "order_intent"));
    }
}

// ============================================================================
// Integration tests for recovery
// ============================================================================

#[test]
fn test_recovery_without_feature() {
    // When feature is disabled, incomplete intents should return ManualReview
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_recovery.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
    }

    let (_state, action) = reconstruct_state(&path).unwrap();

    #[cfg(not(feature = "write_ahead_logging"))]
    {
        // Without feature, incomplete intents should require manual review
        assert_eq!(action, RecoveryAction::ManualReview);
    }

    #[cfg(feature = "write_ahead_logging")]
    {
        // With feature, incomplete intents should trigger reconciliation
        assert_eq!(action, RecoveryAction::Resume);
    }
}

#[test]
fn test_recovery_with_feature() {
    // When feature is enabled, incomplete intents should return Resume
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("test_recovery.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
    }

    let (_state, action) = reconstruct_state(&path).unwrap();

    #[cfg(feature = "write_ahead_logging")]
    {
        // With feature, incomplete intents should trigger reconciliation
        assert_eq!(action, RecoveryAction::Resume);
    }

    #[cfg(not(feature = "write_ahead_logging"))]
    {
        // Without feature, incomplete intents should require manual review
        assert_eq!(action, RecoveryAction::ManualReview);
    }
}

// ============================================================================
// Backward compatibility tests
// ============================================================================

#[test]
fn test_parse_old_audit_log_with_new_code() {
    // Verify that old audit logs (without OrderIntent) parse correctly with new code
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old_format.jsonl");

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
            serde_json::json!({
                "positions": [{"symbol": "AAPL", "qty": 100, "avg_cost": 150.0}],
                "equity": 100000.0,
            }),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::DiffComputed,
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
            Checkpoint::OrderSubmitted,
            4,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
            }),
        )
        .unwrap();
    }

    let (state, _action) = reconstruct_state(&path).unwrap();
    assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
    assert_eq!(state.sequence_number, 4);
    assert_eq!(state.orders.len(), 1);
    assert!(state.orders[0].submitted);
    assert_eq!(state.orders[0].ibkr_id, 12345);
}

#[test]
fn test_parse_new_audit_log_with_old_code() {
    // Verify that new audit logs (with OrderIntent) parse correctly
    // This test simulates forward compatibility - old code reading new logs
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new_format.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderSubmitted,
            3,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
            }),
        )
        .unwrap();
    }

    let (state, _action) = reconstruct_state(&path).unwrap();
    assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
    assert_eq!(state.sequence_number, 3);
    assert_eq!(state.orders.len(), 1);
    assert!(state.orders[0].submitted);
    assert_eq!(state.orders[0].ibkr_id, 12345);
    assert_eq!(state.orders[0].client_order_id, Some("test_client_order_123".to_string()));
}

#[test]
fn test_mixed_checkpoint_sequences() {
    // Verify that mixed checkpoint sequences (old and new formats) parse correctly
    // Note: Multiple DiffComputed events in a single log will result in only the last one's orders being kept
    // This is expected behavior - reconstruction shows state at the last checkpoint
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("mixed_format.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        // Old format: RunStarted -> DiffComputed -> OrderSubmitted (no OrderIntent)
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::DiffComputed,
            2,
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
            Checkpoint::OrderSubmitted,
            3,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
            }),
        )
        .unwrap();
        // New format: DiffComputed -> OrderIntent -> OrderSubmitted
        log.log_checkpoint(
            Checkpoint::DiffComputed,
            4,
            serde_json::json!({
                "orders": [{
                    "symbol": "MSFT",
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "description": "test"
                }]
            }),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            5,
            serde_json::json!({
                "symbol": "MSFT",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_456",
            }),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderSubmitted,
            6,
            serde_json::json!({
                "symbol": "MSFT",
                "ibkr_id": 67890,
            }),
        )
        .unwrap();
    }

    let (state, _action) = reconstruct_state(&path).unwrap();
    assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
    assert_eq!(state.sequence_number, 6);
    // Only the last DiffComputed's orders are kept (MSFT)
    assert_eq!(state.orders.len(), 1);
    // Order from new format - has client_order_id
    assert!(state.orders[0].submitted);
    assert_eq!(state.orders[0].ibkr_id, 67890);
    assert_eq!(state.orders[0].client_order_id, Some("test_client_order_456".to_string()));
}

// ============================================================================
// Golden fixture tests
// ============================================================================

#[test]
fn test_golden_fixture_old_format() {
    // Test parsing of old_format.jsonl fixture
    let fixture_path = std::path::PathBuf::from("tests/fixtures/old_format.jsonl");

    // Create the fixture if it doesn't exist
    if !fixture_path.exists() {
        let content = r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"positions_fetched","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"PositionsFetched","positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":15000.0}
{"event":"diff_computed","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"DiffComputed","orders":[{"symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"description":"rebalance order"}]}
{"event":"risk_check_passed","ts":"2024-01-15T10:00:03Z","sequence_number":4,"checkpoint":"RiskCheckPassed","checks":[{"name":"max_position","status":"Passed","detail":"Position within limits"}]}
{"event":"order_submitted","ts":"2024-01-15T10:00:05Z","sequence_number":5,"checkpoint":"OrderSubmitted","symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"ibkr_id":12345}
{"event":"order_filled","ts":"2024-01-15T10:00:06Z","sequence_number":6,"checkpoint":"OrderFilled","symbol":"AAPL","ibkr_id":12345,"filled":50,"avg_price":155.0,"commission":1.0,"status":"Filled"}"#;
        std::fs::write(&fixture_path, content).unwrap();
    }

    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");
    assert_eq!(events.len(), 6);

    // Verify no OrderIntent in old format
    assert!(!events.iter().any(|e| e.event == "order_intent"));
    // Verify OrderSubmitted is present
    assert!(events.iter().any(|e| e.event == "order_submitted"));

    let (_state, _action) = reconstruct_state(&fixture_path).unwrap();
    // Old format should parse correctly
}

#[test]
fn test_golden_fixture_new_format() {
    // Test parsing of new_format.jsonl fixture
    let fixture_path = std::path::PathBuf::from("tests/fixtures/new_format.jsonl");

    // Create the fixture if it doesn't exist
    if !fixture_path.exists() {
        let content = r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"positions_fetched","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"PositionsFetched","positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":15000.0}
{"event":"diff_computed","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"DiffComputed","orders":[{"symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"description":"rebalance order"}]}
{"event":"risk_check_passed","ts":"2024-01-15T10:00:03Z","sequence_number":4,"checkpoint":"RiskCheckPassed","checks":[{"name":"max_position","status":"Passed","detail":"Position within limits"}]}
{"event":"order_intent","ts":"2024-01-15T10:00:04Z","sequence_number":5,"checkpoint":"OrderIntent","symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"client_order_id":"client-123","timestamp":"2024-01-15T10:00:04Z","target_spec_reference":"target.json","execution_context":"cron"}
{"event":"order_submitted","ts":"2024-01-15T10:00:05Z","sequence_number":6,"checkpoint":"OrderSubmitted","symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"ibkr_id":12345}
{"event":"order_filled","ts":"2024-01-15T10:00:06Z","sequence_number":7,"checkpoint":"OrderFilled","symbol":"AAPL","ibkr_id":12345,"filled":50,"avg_price":155.0,"commission":1.0,"status":"Filled"}"#;
        std::fs::write(&fixture_path, content).unwrap();
    }

    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");
    assert_eq!(events.len(), 7);

    // Verify OrderIntent is present in new format
    assert!(events.iter().any(|e| e.event == "order_intent"));
    // Verify OrderSubmitted is present
    assert!(events.iter().any(|e| e.event == "order_submitted"));

    let (_state, _action) = reconstruct_state(&fixture_path).unwrap();
    // New format should parse correctly
}

#[test]
fn test_golden_fixture_mixed_format() {
    // Test parsing of mixed_format.jsonl fixture
    let fixture_path = std::path::PathBuf::from("tests/fixtures/mixed_format.jsonl");

    // Create the fixture if it doesn't exist
    if !fixture_path.exists() {
        let content = r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"positions_fetched","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"PositionsFetched","positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":15000.0}
{"event":"diff_computed","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"DiffComputed","orders":[{"symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"description":"rebalance order"}]}
{"event":"risk_check_passed","ts":"2024-01-15T10:00:03Z","sequence_number":4,"checkpoint":"RiskCheckPassed","checks":[{"name":"max_position","status":"Passed","detail":"Position within limits"}]}
{"event":"order_submitted","ts":"2024-01-15T10:00:05Z","sequence_number":5,"checkpoint":"OrderSubmitted","symbol":"AAPL","action":"Buy","shares":50,"limit":160.0,"ibkr_id":12345}
{"event":"order_filled","ts":"2024-01-15T10:00:06Z","sequence_number":6,"checkpoint":"OrderFilled","symbol":"AAPL","ibkr_id":12345,"filled":50,"avg_price":155.0,"commission":1.0,"status":"Filled"}
{"event":"run_started","ts":"2024-01-15T11:00:00Z","sequence_number":7,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"positions_fetched","ts":"2024-01-15T11:00:01Z","sequence_number":8,"checkpoint":"PositionsFetched","positions":[{"symbol":"MSFT","qty":50,"avg_cost":200.0}],"equity":15000.0}
{"event":"diff_computed","ts":"2024-01-15T11:00:02Z","sequence_number":9,"checkpoint":"DiffComputed","orders":[{"symbol":"MSFT","action":"Buy","shares":25,"limit":210.0,"description":"rebalance order"}]}
{"event":"risk_check_passed","ts":"2024-01-15T11:00:03Z","sequence_number":10,"checkpoint":"RiskCheckPassed","checks":[{"name":"max_position","status":"Passed","detail":"Position within limits"}]}
{"event":"order_intent","ts":"2024-01-15T11:00:04Z","sequence_number":11,"checkpoint":"OrderIntent","symbol":"MSFT","action":"Buy","shares":25,"limit":210.0,"client_order_id":"client-456","timestamp":"2024-01-15T11:00:04Z","target_spec_reference":"target.json","execution_context":"cron"}
{"event":"order_submitted","ts":"2024-01-15T11:00:05Z","sequence_number":12,"checkpoint":"OrderSubmitted","symbol":"MSFT","action":"Buy","shares":25,"limit":210.0,"ibkr_id":67890}
{"event":"order_filled","ts":"2024-01-15T11:00:06Z","sequence_number":13,"checkpoint":"OrderFilled","symbol":"MSFT","ibkr_id":67890,"filled":25,"avg_price":205.0,"commission":1.0,"status":"Filled"}"#;
        std::fs::write(&fixture_path, content).unwrap();
    }

    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");
    assert_eq!(events.len(), 13);

    // Verify mixed format: first run has no OrderIntent, second run has OrderIntent
    let first_run_events: Vec<_> = events.iter().filter(|e| e.sequence_number.unwrap_or(0) <= 6).collect();
    let second_run_events: Vec<_> = events.iter().filter(|e| e.sequence_number.unwrap_or(0) > 6).collect();

    assert!(!first_run_events.iter().any(|e| e.event == "order_intent"));
    assert!(second_run_events.iter().any(|e| e.event == "order_intent"));

    let (_state, _action) = reconstruct_state(&fixture_path).unwrap();
    // Mixed format should parse correctly
}

// ============================================================================
// Edge case tests
// ============================================================================

#[test]
fn test_feature_enabled_old_format_audit_log() {
    // Feature enabled but audit log has old format (no OrderIntent)
    // Should parse correctly
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("old_format_with_feature.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::DiffComputed,
            2,
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
            Checkpoint::OrderSubmitted,
            3,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
            }),
        )
        .unwrap();
    }

    let (state, _action) = reconstruct_state(&path).unwrap();
    assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
    assert_eq!(state.orders.len(), 1);
    assert_eq!(state.orders[0].client_order_id, None); // Old format
}

#[test]
fn test_feature_disabled_new_format_audit_log() {
    // Feature disabled but audit log has new format (with OrderIntent)
    // Should parse correctly
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("new_format_without_feature.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderSubmitted,
            3,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
            }),
        )
        .unwrap();
    }

    let (state, _action) = reconstruct_state(&path).unwrap();
    assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
    assert_eq!(state.orders.len(), 1);
    assert_eq!(state.orders[0].client_order_id, Some("test_client_order_123".to_string()));
}

#[test]
fn test_incomplete_intent_without_feature() {
    // Incomplete intent without feature flag
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incomplete_without_feature.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
    }

    let (_state, action) = reconstruct_state(&path).unwrap();

    #[cfg(not(feature = "write_ahead_logging"))]
    {
        assert_eq!(action, RecoveryAction::ManualReview);
    }

    #[cfg(feature = "write_ahead_logging")]
    {
        assert_eq!(action, RecoveryAction::Resume);
    }
}

#[test]
fn test_incomplete_intent_with_feature() {
    // Incomplete intent with feature flag
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("incomplete_with_feature.jsonl");

    {
        let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
        log.log_checkpoint(
            Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        )
        .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            2,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "test_client_order_123",
            }),
        )
        .unwrap();
    }

    let (_state, action) = reconstruct_state(&path).unwrap();

    #[cfg(feature = "write_ahead_logging")]
    {
        assert_eq!(action, RecoveryAction::Resume);
    }

    #[cfg(not(feature = "write_ahead_logging"))]
    {
        assert_eq!(action, RecoveryAction::ManualReview);
    }
}
