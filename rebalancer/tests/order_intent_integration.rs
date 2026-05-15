//! Integration tests for OrderIntent and OrderFailed checkpoint variants.

use nanobook_rebalancer::audit::{parse_audit_events, validate_checkpoints_from_parsed, AuditLog, Checkpoint};
use std::path::PathBuf;

/// Test that intent_only.jsonl (crash scenario) parses correctly.
#[test]
fn fixture_intent_only_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 5, "Expected 5 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_fetched");
    assert_eq!(events[2].event, "diff_computed");
    assert_eq!(events[3].event, "risk_check_passed");
    assert_eq!(events[4].event, "order_intent");

    // Verify all have sequence numbers
    for (i, event) in events.iter().enumerate() {
        assert_eq!(
            event.sequence_number,
            Some((i + 1) as u64),
            "Event {} missing sequence number",
            i
        );
    }

    // Verify OrderIntent has all required fields
    let intent_event = &events[4];
    assert_eq!(intent_event.event, "order_intent");
    // Check that data fields exist (using get() which returns Option<&Value>)
    assert!(intent_event.data.get("symbol").is_some());
    assert!(intent_event.data.get("action").is_some());
    assert!(intent_event.data.get("client_order_id").is_some());
}

/// Test that intent_only.jsonl validates correctly (should pass with warning about incomplete intent).
#[test]
fn fixture_intent_only_validates_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_only.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should succeed (soft validation allows incomplete intents)
    let result = validate_checkpoints_from_parsed(&events);
    assert!(result.is_ok(), "Validation should succeed for incomplete intent");
}

/// Test that intent_success.jsonl parses correctly.
#[test]
fn fixture_intent_success_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_success.jsonl");
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

/// Test that intent_success.jsonl validates correctly.
#[test]
fn fixture_intent_success_validates_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_success.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should succeed
    let result = validate_checkpoints_from_parsed(&events);
    assert!(result.is_ok(), "Validation should succeed for successful intent");
}

/// Test that intent_failure.jsonl parses correctly.
#[test]
fn fixture_intent_failure_parses_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_failure.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    assert_eq!(events.len(), 6, "Expected 6 events in fixture");

    // Verify checkpoints are in correct order
    assert_eq!(events[0].event, "run_started");
    assert_eq!(events[1].event, "positions_fetched");
    assert_eq!(events[2].event, "diff_computed");
    assert_eq!(events[3].event, "risk_check_passed");
    assert_eq!(events[4].event, "order_intent");
    assert_eq!(events[5].event, "order_failed");

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
    let failed_event = &events[5];
    assert_eq!(failed_event.event, "order_failed");
    // Check that data fields exist
    assert!(failed_event.data.get("error_type").is_some());
    assert!(failed_event.data.get("error_message").is_some());
}

/// Test that intent_failure.jsonl validates correctly.
#[test]
fn fixture_intent_failure_validates_correctly() {
    let fixture_path = PathBuf::from("tests/fixtures/intent_failure.jsonl");
    let events = parse_audit_events(&fixture_path).expect("Failed to parse fixture");

    // Validation should succeed
    let result = validate_checkpoints_from_parsed(&events);
    assert!(result.is_ok(), "Validation should succeed for failed intent");
}

/// Test that checkpoints can be round-tripped through the audit log.
#[test]
fn checkpoint_roundtrip_order_intent() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            Checkpoint::OrderIntent,
            5,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "shares": 50,
                "limit": 160.0,
                "client_order_id": "client-123",
                "timestamp": "2024-01-15T10:00:04Z",
                "target_spec_reference": "target.json",
                "execution_context": "cron"
            }),
        )
        .unwrap();
    }

    // Parse back
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.event, "order_intent");
    assert_eq!(event.sequence_number, Some(5));
    assert!(event.checkpoint.is_some());

    // Verify checkpoint can be parsed from event name
    let checkpoint = Checkpoint::from_event_name(&event.event);
    assert_eq!(checkpoint, Some(Checkpoint::OrderIntent));

    // Verify the checkpoint can be converted back to event name
    if let Some(cp) = checkpoint {
        assert_eq!(cp.as_event_name(), "order_intent");
    }
}

/// Test that checkpoints can be round-tripped through the audit log for OrderFailed.
#[test]
fn checkpoint_roundtrip_order_failed() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            Checkpoint::OrderFailed,
            6,
            serde_json::json!({
                "error_type": "ConnectionError",
                "error_message": "Failed to connect to broker",
                "context": "during order submission"
            }),
        )
        .unwrap();
    }

    // Parse back
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);

    let event = &events[0];
    assert_eq!(event.event, "order_failed");
    assert_eq!(event.sequence_number, Some(6));
    assert!(event.checkpoint.is_some());

    // Verify checkpoint can be parsed from event name
    let checkpoint = Checkpoint::from_event_name(&event.event);
    assert_eq!(checkpoint, Some(Checkpoint::OrderFailed));

    // Verify the checkpoint can be converted back to event name
    if let Some(cp) = checkpoint {
        assert_eq!(cp.as_event_name(), "order_failed");
    }
}

/// Test that the full sequence with OrderIntent validates correctly.
#[test]
fn full_sequence_with_order_intent_validates() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(Checkpoint::RunStarted, 1, serde_json::json!({}))
            .unwrap();
        log.log_checkpoint(
            Checkpoint::PositionsFetched,
            2,
            serde_json::json!({}),
        )
        .unwrap();
        log.log_checkpoint(Checkpoint::DiffComputed, 3, serde_json::json!({}))
            .unwrap();
        log.log_checkpoint(
            Checkpoint::RiskCheckPassed,
            4,
            serde_json::json!({}),
        )
        .unwrap();
        log.log_checkpoint(Checkpoint::OrderIntent, 5, serde_json::json!({}))
            .unwrap();
        log.log_checkpoint(
            Checkpoint::OrderSubmitted,
            6,
            serde_json::json!({}),
        )
        .unwrap();
    }

    let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
    assert!(log.validate_checkpoints().is_ok());
}
