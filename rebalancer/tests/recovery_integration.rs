//! Integration tests for F9 crash recovery.

use nanobook_rebalancer::audit::AuditLog;
use nanobook_rebalancer::recovery::{reconstruct_state, RecoveryAction};

/// Test recovery from crash at each checkpoint.
#[test]
fn test_recovery_from_run_started() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at run_started checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::RunStarted);
    assert_eq!(state.sequence_number, 1);
    assert!(!state.run_completed);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn test_recovery_from_positions_fetched() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at positions_fetched checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
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
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::PositionsFetched);
    assert_eq!(state.sequence_number, 2);
    assert_eq!(state.positions.len(), 1);
    assert_eq!(state.positions[0].symbol.as_str(), "AAPL");
    assert_eq!(state.positions[0].quantity, 100);
    assert!(!state.run_completed);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn test_recovery_from_diff_computed() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at diff_computed checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
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
        ).unwrap();
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
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::DiffComputed);
    assert_eq!(state.sequence_number, 3);
    assert_eq!(state.orders.len(), 1);
    assert_eq!(state.orders[0].symbol.as_str(), "AAPL");
    assert_eq!(state.orders[0].shares, 50);
    assert!(!state.orders[0].submitted);
    assert!(!state.run_completed);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn test_recovery_from_order_submitted() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at order_submitted checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
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
        ).unwrap();
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
        ).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::OrderSubmitted,
            4,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "ibkr_id": 12345
            }),
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::OrderSubmitted);
    assert_eq!(state.sequence_number, 4);
    assert_eq!(state.orders.len(), 1);
    assert_eq!(state.orders[0].ibkr_id, 12345);
    assert!(state.orders[0].submitted);
    assert!(!state.orders[0].filled);
    assert!(!state.run_completed);
    assert_eq!(action, RecoveryAction::ManualReview);
}

#[test]
fn test_recovery_from_order_filled() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at order_filled checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
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
        ).unwrap();
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
        ).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::OrderSubmitted,
            4,
            serde_json::json!({
                "symbol": "AAPL",
                "action": "Buy",
                "ibkr_id": 12345
            }),
        ).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::OrderFilled,
            5,
            serde_json::json!({
                "symbol": "AAPL",
                "ibkr_id": 12345,
                "filled": 50,
                "avg_price": 155.0,
                "commission": 1.0,
                "status": "Filled"
            }),
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::OrderFilled);
    assert_eq!(state.sequence_number, 5);
    assert_eq!(state.orders.len(), 1);
    assert!(state.orders[0].submitted);
    assert!(state.orders[0].filled);
    assert!(!state.run_completed);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn test_recovery_from_run_completed() {
    let dir = tempfile::tempdir().unwrap();
    let audit_path = dir.path().join("audit.jsonl");
    let workdir = dir.path();

    // Simulate a crash at run_completed checkpoint
    {
        let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunStarted,
            1,
            serde_json::json!({"target": "test"}),
        ).unwrap();
        log.log_checkpoint(
            nanobook_rebalancer::audit::Checkpoint::RunCompleted,
            2,
            serde_json::json!({
                "submitted": 1,
                "filled": 1,
                "failed": 0
            }),
        ).unwrap();
    }

    // Recover state
    let (state, action) = reconstruct_state(&audit_path).unwrap();

    assert_eq!(state.checkpoint, nanobook_rebalancer::audit::Checkpoint::RunCompleted);
    assert_eq!(state.sequence_number, 2);
    assert!(state.run_completed);
    assert_eq!(action, RecoveryAction::Restart);
}

#[test]
fn test_checkpoint_coverage_all_checkpoints() {
    let checkpoints = vec![
        nanobook_rebalancer::audit::Checkpoint::RunStarted,
        nanobook_rebalancer::audit::Checkpoint::PositionsFetched,
        nanobook_rebalancer::audit::Checkpoint::DiffComputed,
        nanobook_rebalancer::audit::Checkpoint::RiskCheckPassed,
        nanobook_rebalancer::audit::Checkpoint::OrderSubmitted,
        nanobook_rebalancer::audit::Checkpoint::OrderFilled,
        nanobook_rebalancer::audit::Checkpoint::RunCompleted,
    ];

    for (i, checkpoint) in checkpoints.iter().enumerate() {
        let dir = tempfile::tempdir().unwrap();
        let audit_path = dir.path().join("audit.jsonl");
        let workdir = dir.path();

        // Simulate crash at this checkpoint
        {
            let mut log = AuditLog::open_in(&audit_path, workdir).unwrap();
            log.log_checkpoint(
                nanobook_rebalancer::audit::Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
            
            // Add the specific checkpoint
            log.log_checkpoint(
                *checkpoint,
                (i + 2) as u64,
                serde_json::json!({"test": "data"}),
            ).unwrap();
        }

        // Verify recovery works
        let (state, _action) = reconstruct_state(&audit_path).unwrap();
        assert_eq!(state.checkpoint, *checkpoint, "Checkpoint mismatch for {:?}", checkpoint);
        assert_eq!(state.sequence_number, (i + 2) as u64);
    }
}
