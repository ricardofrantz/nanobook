//! Integration tests for Phase 1.6C account-summary and cancellation write-ahead logging.

#![cfg(feature = "write_ahead_logging")]

use std::cell::RefCell;
use std::time::Duration;

use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::types::{Account, Position, Quote};
use nanobook_broker::{BrokerSide, ClientOrderId};
use nanobook_rebalancer::audit::{
    AuditLog, max_checkpoint_sequence, parse_audit_events, validate_checkpoints_from_parsed,
};
use nanobook_rebalancer::broker::BrokerGateway;
use nanobook_rebalancer::execution::{
    cancel_order_with_write_ahead, fetch_account_summary_with_write_ahead,
};
use nanobook_rebalancer::recovery::{RecoveryAction, reconstruct_state};

struct MockBroker {
    fail_account: bool,
    fail_cancel: bool,
    account_calls: RefCell<usize>,
    cancel_calls: RefCell<usize>,
}

impl MockBroker {
    fn new() -> Self {
        Self {
            fail_account: false,
            fail_cancel: false,
            account_calls: RefCell::new(0),
            cancel_calls: RefCell::new(0),
        }
    }

    fn with_account_error() -> Self {
        Self {
            fail_account: true,
            ..Self::new()
        }
    }

    fn with_cancel_error() -> Self {
        Self {
            fail_cancel: true,
            ..Self::new()
        }
    }

    fn account_calls(&self) -> usize {
        *self.account_calls.borrow()
    }

    fn cancel_calls(&self) -> usize {
        *self.cancel_calls.borrow()
    }
}

impl BrokerGateway for MockBroker {
    fn account_summary(&self) -> Result<Account, BrokerError> {
        *self.account_calls.borrow_mut() += 1;
        if self.fail_account {
            return Err(BrokerError::Connection(
                "account summary unavailable".into(),
            ));
        }
        Ok(Account {
            equity_cents: 1_500_000,
            buying_power_cents: 1_200_000,
            cash_cents: 1_000_000,
            gross_position_value_cents: 500_000,
        })
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        Ok(Vec::new())
    }

    fn prices(&self, _symbols: &[Symbol]) -> Result<Vec<(Symbol, i64)>, BrokerError> {
        Ok(Vec::new())
    }

    fn quotes(&self, _symbols: &[Symbol]) -> Result<Vec<Quote>, BrokerError> {
        Ok(Vec::new())
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

    fn cancel_order(&self, _order_id: u64) -> Result<(), BrokerError> {
        *self.cancel_calls.borrow_mut() += 1;
        if self.fail_cancel {
            return Err(BrokerError::CancelReject {
                order_id: 42,
                reason: "already filled".into(),
            });
        }
        Ok(())
    }
}

#[test]
fn account_summary_success_logs_intent_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("account_success.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
    let broker = MockBroker::new();
    let mut sequence = 0;

    let summary =
        fetch_account_summary_with_write_ahead(&broker, &mut audit, &mut sequence, "target.json")
            .unwrap();

    assert_eq!(summary.equity_cents, 1_500_000);
    assert_eq!(broker.account_calls(), 1);
    assert_eq!(sequence, 2);

    drop(audit);
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "account_summary_intent");
    assert_eq!(events[0].sequence_number, Some(1));
    assert_eq!(events[1].event, "account_summary_result");
    assert_eq!(events[1].sequence_number, Some(2));
    assert_eq!(events[1].data["equity"], 15_000.0);
    assert_eq!(events[1].data["cash"], 10_000.0);
}

#[test]
fn account_summary_failure_leaves_incomplete_intent() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("account_failure.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
    let broker = MockBroker::with_account_error();
    let mut sequence = 0;

    let result =
        fetch_account_summary_with_write_ahead(&broker, &mut audit, &mut sequence, "target.json");

    assert!(result.is_err());
    assert_eq!(broker.account_calls(), 1);
    assert_eq!(sequence, 1);

    drop(audit);
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].event, "account_summary_intent");
}

#[test]
fn cancel_success_logs_intent_and_result() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("cancel_success.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
    let broker = MockBroker::new();
    let mut sequence = 10;

    cancel_order_with_write_ahead(&broker, &mut audit, &mut sequence, 42, "operator_requested")
        .unwrap();

    assert_eq!(broker.cancel_calls(), 1);
    assert_eq!(sequence, 12);

    drop(audit);
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "cancel_intent");
    assert_eq!(events[0].sequence_number, Some(11));
    assert_eq!(events[0].data["order_id"], 42);
    assert_eq!(events[1].event, "cancel_result");
    assert_eq!(events[1].sequence_number, Some(12));
    assert_eq!(events[1].data["success"], true);
}

#[test]
fn cancel_failure_logs_negative_result() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("cancel_failure.jsonl");
    let mut audit = AuditLog::open_in(&audit_path, dir.path()).unwrap();
    let broker = MockBroker::with_cancel_error();
    let mut sequence = 0;

    let result =
        cancel_order_with_write_ahead(&broker, &mut audit, &mut sequence, 42, "timeout_cleanup");

    assert!(result.is_err());
    assert_eq!(broker.cancel_calls(), 1);
    assert_eq!(sequence, 2);

    drop(audit);
    let events = parse_audit_events(&audit_path).unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event, "cancel_intent");
    assert_eq!(events[1].event, "cancel_result");
    assert_eq!(events[1].data["success"], false);
    assert!(
        events[1].data["error_message"]
            .as_str()
            .unwrap()
            .contains("already filled")
    );
}

#[test]
fn complete_coverage_fixture_validates() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/complete_coverage.jsonl");
    let events = parse_audit_events(&fixture_path).unwrap();

    assert!(
        events
            .iter()
            .any(|event| event.event == "account_summary_intent")
    );
    assert!(events.iter().any(|event| event.event == "positions_intent"));
    assert!(events.iter().any(|event| event.event == "quotes_intent"));
    assert!(events.iter().any(|event| event.event == "order_intent"));
    validate_checkpoints_from_parsed(&events).unwrap();
}

#[test]
fn recovery_detects_incomplete_account_summary_intent() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/account_summary_intent_only.jsonl");
    let (state, action) = reconstruct_state(&fixture_path).unwrap();

    assert!(state.account_summary_intent_logged);
    assert!(!state.account_summary_result_logged);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn recovery_requires_manual_review_for_incomplete_cancel_intent() {
    let fixture_path = std::path::PathBuf::from("tests/fixtures/cancel_intent_only.jsonl");
    let (state, action) = reconstruct_state(&fixture_path).unwrap();

    assert!(state.cancel_intent_logged);
    assert!(!state.cancel_result_logged);
    assert_eq!(action, RecoveryAction::ManualReview);
}

#[test]
fn recovery_requires_manual_review_for_failed_cancel_result() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("cancel_failed_recovery.jsonl");
    std::fs::write(
        &audit_path,
        r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"cancel_intent","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"CancelIntent","order_id":42,"cancellation_reason":"operator_requested","timestamp":"2024-01-15T10:00:01Z"}
{"event":"cancel_result","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"CancelResult","order_id":42,"success":false,"error_message":"already filled"}
"#,
    )
    .unwrap();

    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert!(state.cancel_result_logged);
    assert!(state.cancel_failed);
    assert_eq!(action, RecoveryAction::ManualReview);
}

#[test]
fn recovery_detects_latest_account_summary_intent_without_result() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("latest_account_incomplete.jsonl");
    std::fs::write(
        &audit_path,
        r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"account_summary_intent","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"AccountSummaryIntent","timestamp":"2024-01-15T10:00:01Z","target_spec_reference":"target.json"}
{"event":"account_summary_result","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"AccountSummaryResult","equity":15000.0,"cash":10000.0}
{"event":"account_summary_intent","ts":"2024-01-15T10:00:03Z","sequence_number":4,"checkpoint":"AccountSummaryIntent","timestamp":"2024-01-15T10:00:03Z","target_spec_reference":"target.json"}
"#,
    )
    .unwrap();

    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.last_account_summary_intent_sequence, Some(4));
    assert_eq!(state.last_account_summary_result_sequence, Some(3));
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn max_checkpoint_sequence_reads_existing_audit_tail() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("max_sequence.jsonl");
    std::fs::write(
        &audit_path,
        r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":10,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"no_rebalance_needed","ts":"2024-01-15T10:00:01Z"}
{"event":"run_completed","ts":"2024-01-15T10:00:02Z","sequence_number":12,"checkpoint":"RunCompleted","submitted":0,"filled":0,"failed":0}
"#,
    )
    .unwrap();

    assert_eq!(max_checkpoint_sequence(&audit_path).unwrap(), 12);
}

#[test]
fn no_rebalance_checkpoint_sequence_validates_as_completed_run() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("no_rebalance_completed.jsonl");
    std::fs::write(
        &audit_path,
        r#"{"event":"run_started","ts":"2024-01-15T10:00:00Z","sequence_number":1,"checkpoint":"RunStarted","target_file":"target.json","account":"U1234567"}
{"event":"account_summary_intent","ts":"2024-01-15T10:00:01Z","sequence_number":2,"checkpoint":"AccountSummaryIntent","timestamp":"2024-01-15T10:00:01Z","target_spec_reference":"target.json"}
{"event":"account_summary_result","ts":"2024-01-15T10:00:02Z","sequence_number":3,"checkpoint":"AccountSummaryResult","equity":15000.0,"cash":10000.0}
{"event":"positions_intent","ts":"2024-01-15T10:00:03Z","sequence_number":4,"checkpoint":"PositionsIntent","timestamp":"2024-01-15T10:00:03Z","target_spec_reference":"target.json"}
{"event":"positions_result","ts":"2024-01-15T10:00:04Z","sequence_number":5,"checkpoint":"PositionsResult","positions":[],"equity":15000.0}
{"event":"quotes_intent","ts":"2024-01-15T10:00:05Z","sequence_number":6,"checkpoint":"QuotesIntent","symbols":[],"staleness_threshold_sec":60,"timestamp":"2024-01-15T10:00:05Z","target_spec_reference":"target.json"}
{"event":"quotes_result","ts":"2024-01-15T10:00:06Z","sequence_number":7,"checkpoint":"QuotesResult","quotes":[]}
{"event":"diff_computed","ts":"2024-01-15T10:00:07Z","sequence_number":8,"checkpoint":"DiffComputed","orders":[]}
{"event":"no_rebalance_needed","ts":"2024-01-15T10:00:08Z"}
{"event":"run_completed","ts":"2024-01-15T10:00:09Z","sequence_number":9,"checkpoint":"RunCompleted","submitted":0,"filled":0,"failed":0}
"#,
    )
    .unwrap();

    let events = parse_audit_events(&audit_path).unwrap();
    validate_checkpoints_from_parsed(&events).unwrap();
    let (_state, action) = reconstruct_state(&audit_path).unwrap();
    assert_eq!(action, RecoveryAction::Restart);
}
