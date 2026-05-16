//! Integration tests for the kill switch functionality.

use std::fs;

use nanobook_rebalancer::audit::{
    AuditLog, log_kill_completed, log_kill_completed_with_summary, log_kill_requested,
    parse_audit_events,
};

#[test]
fn test_verify_no_dangling_orders_integration() {
    // Test order verification with a realistic audit log
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");

    // Create an audit log with submitted and filled orders
    let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_filled","ts":"2024-01-01T00:00:01Z","symbol":"AAPL","ibkr_id":1,"filled":100,"avg_price":150.00}
{"event":"order_submitted","ts":"2024-01-01T00:00:02Z","symbol":"MSFT","action":"Sell","shares":50,"limit":400.00,"ibkr_id":2}
{"event":"order_filled","ts":"2024-01-01T00:00:03Z","symbol":"MSFT","ibkr_id":2,"filled":50,"avg_price":400.00}"#;
    fs::write(&audit_path, audit_log).unwrap();

    // Verify no dangling orders
    let result = nanobook_rebalancer::kill::verify_no_dangling_orders(&audit_path);
    assert!(result.is_ok());
    let dangling = result.unwrap();
    assert!(dangling.is_empty());
}

#[test]
fn test_verify_dangling_orders_integration() {
    // Test order verification with dangling orders
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");

    // Create an audit log with a dangling order (submitted but not filled)
    let audit_log = r#"{"event":"order_submitted","ts":"2024-01-01T00:00:00Z","symbol":"AAPL","action":"Buy","shares":100,"limit":150.00,"ibkr_id":1}
{"event":"order_submitted","ts":"2024-01-01T00:00:01Z","symbol":"MSFT","action":"Sell","shares":50,"limit":400.00,"ibkr_id":2}
{"event":"order_filled","ts":"2024-01-01T00:00:02Z","symbol":"AAPL","ibkr_id":1,"filled":100,"avg_price":150.00}"#;
    fs::write(&audit_path, audit_log).unwrap();

    // Verify dangling orders
    let result = nanobook_rebalancer::kill::verify_no_dangling_orders(&audit_path);
    assert!(result.is_ok());
    let dangling = result.unwrap();
    assert_eq!(dangling.len(), 1);
    assert_eq!(dangling[0].symbol, "MSFT");
    assert_eq!(dangling[0].ibkr_id, 2);
}

#[test]
fn test_kill_completed_audit_event_includes_graceful_shutdown_fields() {
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, temp_dir.path()).unwrap();

    log_kill_completed(&mut audit, "graceful", 3, 1.25).unwrap();
    drop(audit);

    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "kill_completed");
    assert_eq!(events[0].data["method"], "graceful");
    assert_eq!(events[0].data["orders_cancelled_count"], 3);
    assert_eq!(events[0].data["duration_seconds"], 1.25);
    assert_eq!(events[0].data["orders_remaining_count"], 0);
    assert_eq!(
        events[0].data["error_messages"].as_array().unwrap().len(),
        0
    );
}

#[test]
fn test_kill_requested_audit_event_includes_method_and_source() {
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, temp_dir.path()).unwrap();

    log_kill_requested(&mut audit, "forceful", "command").unwrap();
    drop(audit);

    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "kill_requested");
    assert_eq!(events[0].data["method"], "forceful");
    assert_eq!(events[0].data["trigger_source"], "command");
}

fn assert_fixture_sequence(path: &str, expected_events: &[&str]) {
    let fixture_path = std::path::PathBuf::from(path);
    let events = parse_audit_events(&fixture_path).unwrap();
    let names: Vec<_> = events.iter().map(|event| event.event.as_str()).collect();
    assert_eq!(names, expected_events);
}

#[test]
fn golden_fixture_phase1_success_parses() {
    assert_fixture_sequence(
        "tests/fixtures/phase1_success.jsonl",
        &[
            "kill_requested",
            "kill_phase1_started",
            "kill_phase1_completed",
            "kill_completed",
        ],
    );
}

#[test]
fn golden_fixture_phase1_timeout_phase2_success_parses() {
    assert_fixture_sequence(
        "tests/fixtures/phase1_timeout_phase2_success.jsonl",
        &[
            "kill_requested",
            "kill_phase1_started",
            "kill_phase2_started",
            "kill_phase2_completed",
            "kill_completed",
        ],
    );
    let events = parse_audit_events(&std::path::PathBuf::from(
        "tests/fixtures/phase1_timeout_phase2_success.jsonl",
    ))
    .unwrap();
    assert_eq!(events[3].data["orders_cancelled_count"], 2);
    assert_eq!(events[3].data["orders_remaining_count"], 0);
}

#[test]
fn golden_fixture_phase2_partial_failure_parses() {
    assert_fixture_sequence(
        "tests/fixtures/phase2_partial_failure.jsonl",
        &[
            "kill_requested",
            "kill_phase1_started",
            "kill_phase2_started",
            "kill_phase2_completed",
            "kill_completed",
        ],
    );
    let events = parse_audit_events(&std::path::PathBuf::from(
        "tests/fixtures/phase2_partial_failure.jsonl",
    ))
    .unwrap();
    assert_eq!(events[3].data["orders_cancelled_count"], 1);
    assert_eq!(events[3].data["orders_remaining_count"], 1);
    assert_eq!(events[3].data["remaining_order_ids"][0], 11);
}

#[test]
fn test_kill_completed_audit_event_includes_remaining_orders_and_errors() {
    let temp_dir = tempfile::tempdir().unwrap();
    let audit_path = temp_dir.path().join("audit.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, temp_dir.path()).unwrap();
    let errors = vec!["order 42 still open".to_string()];

    log_kill_completed_with_summary(&mut audit, "forced", 2, 1, 3.5, &errors).unwrap();
    drop(audit);

    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "kill_completed");
    assert_eq!(events[0].data["method"], "forced");
    assert_eq!(events[0].data["orders_cancelled_count"], 2);
    assert_eq!(events[0].data["orders_remaining_count"], 1);
    assert_eq!(events[0].data["duration_seconds"], 3.5);
    assert_eq!(events[0].data["error_messages"][0], "order 42 still open");
}
