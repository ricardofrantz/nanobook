//! Crash recovery and state reconstruction from audit logs.

use crate::audit::{AuditEvent, Checkpoint};
use crate::error::{Error, Result};
use nanobook::Symbol;
use nanobook_broker::Broker;
use serde::{Deserialize, Serialize};

/// Recovery action to take after a crash.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryAction {
    /// Safe to restart the entire rebalance from the beginning
    Restart,
    /// Resume from the last known good checkpoint
    Resume,
    /// Requires operator intervention to review state and decide on action
    ManualReview,
    /// Rollback submitted orders (if possible) and restart
    Rollback,
}

/// Reconstructed state from audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredState {
    /// The last checkpoint reached before the crash
    pub checkpoint: Checkpoint,
    /// Sequence number of the last checkpoint
    pub sequence_number: u64,
    /// Timestamp of the last checkpoint
    pub timestamp: chrono::DateTime<chrono::Utc>,
    /// Current positions (as of last checkpoint)
    pub positions: Vec<RecoveredPosition>,
    /// Orders submitted (as of last checkpoint)
    pub orders: Vec<RecoveredOrder>,
    /// Total equity (as of last checkpoint)
    pub equity_cents: i64,
    /// Whether the run completed successfully
    pub run_completed: bool,
}

/// Position reconstructed from audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredPosition {
    pub symbol: Symbol,
    pub quantity: i64,
    pub avg_cost_cents: i64,
}

/// Order reconstructed from audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredOrder {
    pub symbol: Symbol,
    pub action: String,
    pub shares: i64,
    pub limit_price_cents: i64,
    pub ibkr_id: i32,
    pub submitted: bool,
    pub filled: bool,
}

/// Discrepancy between broker state and reconstructed state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Discrepancy {
    /// Order exists in broker but not in reconstructed state (orphan order)
    OrphanOrder {
        broker_order_id: u64,
        symbol: String,
        status: String,
    },
    /// Order exists in reconstructed state but not in broker open orders
    MissingOrder {
        symbol: String,
        expected_status: String,
    },
    /// Order status mismatch between broker and reconstructed state
    OrderStatusMismatch {
        symbol: String,
        broker_status: String,
        expected_status: String,
    },
    /// Position mismatch between broker and reconstructed state
    PositionMismatch {
        symbol: String,
        broker_qty: i64,
        expected_qty: i64,
    },
}

/// Report of discrepancies between broker state and reconstructed state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscrepancyReport {
    pub discrepancies: Vec<Discrepancy>,
    pub has_critical_issues: bool,
}

/// Compare broker state with reconstructed state and generate discrepancy report.
pub fn compare_broker_state(
    broker: &dyn Broker,
    recovered_state: &RecoveredState,
) -> Result<DiscrepancyReport> {
    let mut discrepancies = Vec::new();

    // Get broker open orders
    let broker_orders = broker.open_orders()
        .unwrap_or_else(|_| Vec::new());

    // Check for orphan orders (in broker but not in reconstructed state)
    for broker_order in &broker_orders {
        let is_orphan = !recovered_state.orders.iter().any(|recovered_order| {
            recovered_order.ibkr_id as u64 == broker_order.id.0
        });

        if is_orphan {
            discrepancies.push(Discrepancy::OrphanOrder {
                broker_order_id: broker_order.id.0,
                symbol: "UNKNOWN".to_string(), // BrokerOrderStatus doesn't include symbol
                status: format!("{:?}", broker_order.status),
            });
        }
    }

    // Check for missing orders (in reconstructed state but not in broker)
    for recovered_order in &recovered_state.orders {
        if recovered_order.submitted && !recovered_order.filled {
            let is_missing = !broker_orders.iter().any(|broker_order| {
                recovered_order.ibkr_id as u64 == broker_order.id.0
            });

            if is_missing {
                discrepancies.push(Discrepancy::MissingOrder {
                    symbol: recovered_order.symbol.as_str().to_string(),
                    expected_status: "Submitted but not filled".to_string(),
                });
            }
        }
    }

    // Get broker positions
    let broker_positions = broker.positions()
        .unwrap_or_else(|_| Vec::new());

    // Check for position mismatches
    for broker_position in &broker_positions {
        let recovered_position = recovered_state.positions.iter()
            .find(|rp| rp.symbol == broker_position.symbol);

        if let Some(recovered_position) = recovered_position {
            if recovered_position.quantity != broker_position.quantity {
                discrepancies.push(Discrepancy::PositionMismatch {
                    symbol: broker_position.symbol.as_str().to_string(),
                    broker_qty: broker_position.quantity,
                    expected_qty: recovered_position.quantity,
                });
            }
        }
    }

    // Check for positions in recovered state but not in broker
    for recovered_position in &recovered_state.positions {
        let is_missing = !broker_positions.iter().any(|bp| bp.symbol == recovered_position.symbol);
        if is_missing {
            discrepancies.push(Discrepancy::PositionMismatch {
                symbol: recovered_position.symbol.as_str().to_string(),
                broker_qty: 0,
                expected_qty: recovered_position.quantity,
            });
        }
    }

    let has_critical_issues = !discrepancies.is_empty();

    Ok(DiscrepancyReport {
        discrepancies,
        has_critical_issues,
    })
}

impl RecoveredState {
    /// Create an empty recovered state.
    pub fn new() -> Self {
        Self {
            checkpoint: Checkpoint::RunStarted,
            sequence_number: 0,
            timestamp: chrono::Utc::now(),
            positions: Vec::new(),
            orders: Vec::new(),
            equity_cents: 0,
            run_completed: false,
        }
    }

    /// Determine the recovery action based on the reconstructed state.
    pub fn determine_recovery_action(&self) -> RecoveryAction {
        // If run completed successfully, safe to restart
        if self.run_completed {
            return RecoveryAction::Restart;
        }

        // If crashed before any orders were submitted, safe to restart
        if self.orders.is_empty() {
            return RecoveryAction::Restart;
        }

        // If crashed after order submission but before fills, need manual review
        let has_unfilled_orders = self.orders.iter().any(|o| o.submitted && !o.filled);
        if has_unfilled_orders {
            return RecoveryAction::ManualReview;
        }

        // If crashed after all orders filled, safe to restart
        return RecoveryAction::Restart;
    }
}

impl Default for RecoveredState {
    fn default() -> Self {
        Self::new()
    }
}

/// Reconstruct state from audit log.
///
/// This function reads the audit log and reconstructs the state at the
/// time of the last checkpoint. It returns the reconstructed state and
/// the recommended recovery action.
pub fn reconstruct_state(audit_log_path: &std::path::Path) -> Result<(RecoveredState, RecoveryAction)> {
    // Read the audit log file
    let contents = std::fs::read_to_string(audit_log_path)?;

    let mut state = RecoveredState::new();

    // Parse each line and update state
    for (line_num, line) in contents.lines().enumerate() {
        if let Ok(event) = serde_json::from_str::<AuditEvent>(line) {
            // Update state based on event type
            match event.event.as_str() {
                "run_started" => {
                    state.checkpoint = Checkpoint::RunStarted;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;
                    state.run_completed = false;
                }
                "positions_fetched" => {
                    state.checkpoint = Checkpoint::PositionsFetched;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;

                    // Parse positions from event data
                    if let Some(positions_array) = event.data.get("positions") {
                        if let Some(positions) = positions_array.as_array() {
                            state.positions = positions
                                .iter()
                                .filter_map(|p| {
                                    let symbol = p.get("symbol")?.as_str()?;
                                    let qty = p.get("qty")?.as_i64()?;
                                    let avg_cost = p.get("avg_cost")?.as_f64()? as i64;
                                    Symbol::try_new(symbol).map(|sym| RecoveredPosition {
                                        symbol: sym,
                                        quantity: qty,
                                        avg_cost_cents: avg_cost,
                                    })
                                })
                                .collect();
                        }
                    }

                    // Parse equity from event data
                    if let Some(equity) = event.data.get("equity") {
                        if let Some(equity_val) = equity.as_f64() {
                            state.equity_cents = (equity_val * 100.0) as i64;
                        }
                    }
                }
                "diff_computed" => {
                    state.checkpoint = Checkpoint::DiffComputed;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;

                    // Parse orders from event data
                    if let Some(orders_array) = event.data.get("orders") {
                        if let Some(orders) = orders_array.as_array() {
                            state.orders = orders
                                .iter()
                                .filter_map(|o| {
                                    let symbol = o.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                    let action = o.get("action").and_then(|a| a.as_str()).unwrap_or("");
                                    let shares = o.get("shares").and_then(|s| s.as_i64()).unwrap_or(0);
                                    let limit = o.get("limit").and_then(|l| l.as_f64()).unwrap_or(0.0) as i64;
                                    Some(RecoveredOrder {
                                        symbol: Symbol::try_new(symbol).unwrap_or(Symbol::try_new("UNKNOWN").unwrap()),
                                        action: action.to_string(),
                                        shares,
                                        limit_price_cents: limit,
                                        ibkr_id: 0,
                                        submitted: false,
                                        filled: false,
                                    })
                                })
                                .collect();
                        }
                    }
                }
                "risk_check_passed" => {
                    state.checkpoint = Checkpoint::RiskCheckPassed;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;
                }
                "order_submitted" => {
                    state.checkpoint = Checkpoint::OrderSubmitted;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;

                    // Update order submission status
                    if let Some(ibkr_id) = event.data.get("ibkr_id").and_then(|id| id.as_i64()) {
                        if let Some(symbol) = event.data.get("symbol").and_then(|s| s.as_str()) {
                            if let Some(order) = state.orders.iter_mut().find(|o| o.symbol.as_str() == symbol) {
                                order.ibkr_id = ibkr_id as i32;
                                order.submitted = true;
                            }
                        }
                    }
                }
                "order_filled" => {
                    state.checkpoint = Checkpoint::OrderFilled;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;

                    // Update order fill status
                    if let Some(ibkr_id) = event.data.get("ibkr_id").and_then(|id| id.as_i64()) {
                        if let Some(_symbol) = event.data.get("symbol").and_then(|s| s.as_str()) {
                            if let Some(order) = state.orders.iter_mut().find(|o| o.ibkr_id as i64 == ibkr_id) {
                                order.filled = true;
                            }
                        }
                    }
                }
                "run_completed" => {
                    state.checkpoint = Checkpoint::RunCompleted;
                    state.sequence_number = event.sequence_number.unwrap_or(0);
                    state.timestamp = event.ts;
                    state.run_completed = true;
                }
                _ => {
                    // Ignore other events
                }
            }
        } else if line.trim().is_empty() {
            // Skip empty lines
            continue;
        } else {
            return Err(Error::AuditValidation(format!(
                "Failed to parse audit log at line {}: invalid JSON",
                line_num + 1
            )));
        }
    }

    let recovery_action = state.determine_recovery_action();
    Ok((state, recovery_action))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::audit::AuditLog;
    use tempfile::tempdir;

    #[test]
    fn reconstruct_state_empty_log() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty_audit.jsonl");
        std::fs::write(&path, "").unwrap();

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::RunStarted);
        assert_eq!(state.sequence_number, 0);
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[test]
    fn reconstruct_state_with_checkpoints() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_reconstruction.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(
                Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::PositionsFetched,
                2,
                serde_json::json!({
                    "positions": [{"symbol": "AAPL", "qty": 100, "avg_cost": 150.0}],
                    "equity": 100000.0,
                }),
            ).unwrap();
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
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::RiskCheckPassed,
                4,
                serde_json::json!({"checks": []}),
            ).unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::RiskCheckPassed);
        assert_eq!(state.sequence_number, 4);
        assert_eq!(state.positions.len(), 1);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(state.equity_cents, 10000000);
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[test]
    fn reconstruct_state_after_order_submission() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_order_submission.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(
                Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
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
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::OrderSubmitted,
                3,
                serde_json::json!({
                    "symbol": "AAPL",
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "ibkr_id": 12345,
                }),
            ).unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
        assert_eq!(state.sequence_number, 3);
        assert_eq!(state.orders.len(), 1);
        assert!(state.orders[0].submitted);
        assert!(!state.orders[0].filled);
        assert_eq!(state.orders[0].ibkr_id, 12345);
        assert_eq!(action, RecoveryAction::ManualReview);
    }

    #[test]
    fn reconstruct_state_after_order_fill() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_order_fill.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(
                Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
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
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::OrderSubmitted,
                3,
                serde_json::json!({
                    "symbol": "AAPL",
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "ibkr_id": 12345,
                }),
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::OrderFilled,
                4,
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

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderFilled);
        assert_eq!(state.sequence_number, 4);
        assert_eq!(state.orders.len(), 1);
        assert!(state.orders[0].submitted);
        assert!(state.orders[0].filled);
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[test]
    fn reconstruct_state_after_run_completed() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_run_completed.jsonl");

        {
            let mut log = AuditLog::open_in(&path, dir.path()).unwrap();
            log.log_checkpoint(
                Checkpoint::RunStarted,
                1,
                serde_json::json!({"target": "test"}),
            ).unwrap();
            log.log_checkpoint(
                Checkpoint::RunCompleted,
                2,
                serde_json::json!({
                    "submitted": 1,
                    "filled": 1,
                    "failed": 0,
                }),
            ).unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::RunCompleted);
        assert_eq!(state.sequence_number, 2);
        assert!(state.run_completed);
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[test]
    fn compare_broker_state_no_discrepancies() {
        use nanobook_broker::mock::{MockBroker, FillMode};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::RunCompleted,
            sequence_number: 1,
            timestamp: chrono::Utc::now(),
            positions: vec![RecoveredPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 150_00,
            }],
            orders: vec![],
            equity_cents: 100_000_00,
            run_completed: true,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(!report.has_critical_issues);
        assert!(report.discrepancies.is_empty());
    }

    #[test]
    fn compare_broker_state_orphan_order() {
        use nanobook_broker::mock::{MockBroker, FillMode};
        use nanobook_broker::{BrokerOrder, BrokerOrderType, BrokerSide};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediatePartial(0.5))
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        // Submit an order to create an open order in the broker
        let order = BrokerOrder {
            symbol: Symbol::new("AAPL"),
            side: BrokerSide::Buy,
            quantity: 50,
            order_type: BrokerOrderType::Market,
            client_order_id: None,
        };
        broker.submit_order(&order).unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::OrderSubmitted,
            sequence_number: 1,
            timestamp: chrono::Utc::now(),
            positions: vec![RecoveredPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 150_00,
            }],
            orders: vec![], // No orders in recovered state
            equity_cents: 100_000_00,
            run_completed: false,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(report.has_critical_issues);
        assert!(!report.discrepancies.is_empty());
    }

    #[test]
    fn compare_broker_state_position_mismatch() {
        use nanobook_broker::mock::{MockBroker, FillMode};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 150, 150_00) // Different quantity
            .build();
        broker.connect().unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::RunCompleted,
            sequence_number: 1,
            timestamp: chrono::Utc::now(),
            positions: vec![RecoveredPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100, // Different from broker
                avg_cost_cents: 150_00,
            }],
            orders: vec![],
            equity_cents: 100_000_00,
            run_completed: true,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(report.has_critical_issues);
        assert!(!report.discrepancies.is_empty());
    }
}
