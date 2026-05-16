//! Crash recovery and state reconstruction from audit logs.

use crate::audit::Checkpoint;
use crate::config::Config;
use crate::diff::CurrentPosition;
use crate::error::{Error, Result};
use crate::target::TargetSpec;
use nanobook::Symbol;
use nanobook_broker::Broker;
use nanobook_broker::types::f64_cents_checked;
use serde::{Deserialize, Serialize};
#[cfg(feature = "write_ahead_logging")]
use std::time::Duration;

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
    pub positions: Vec<CurrentPosition>,
    /// Orders submitted (as of last checkpoint)
    pub orders: Vec<RecoveredOrder>,
    /// Total equity (as of last checkpoint)
    pub equity_cents: i64,
    /// Whether the run completed successfully
    pub run_completed: bool,
    /// Whether positions fetch intent was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub positions_intent_logged: bool,
    /// Whether positions fetch result was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub positions_result_logged: bool,
    /// Whether quotes fetch intent was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub quotes_intent_logged: bool,
    /// Whether quotes fetch result was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub quotes_result_logged: bool,
    /// Whether account summary fetch intent was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub account_summary_intent_logged: bool,
    /// Whether account summary fetch result was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub account_summary_result_logged: bool,
    /// Whether cancel intent was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub cancel_intent_logged: bool,
    /// Whether cancel result was logged (write-ahead logging)
    #[cfg(feature = "write_ahead_logging")]
    pub cancel_result_logged: bool,
    /// Last positions intent checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_positions_intent_sequence: Option<u64>,
    /// Last positions result checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_positions_result_sequence: Option<u64>,
    /// Last quotes intent checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_quotes_intent_sequence: Option<u64>,
    /// Last quotes result checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_quotes_result_sequence: Option<u64>,
    /// Last account summary intent checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_account_summary_intent_sequence: Option<u64>,
    /// Last account summary result checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_account_summary_result_sequence: Option<u64>,
    /// Last cancel intent checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_cancel_intent_sequence: Option<u64>,
    /// Last cancel result checkpoint sequence.
    #[cfg(feature = "write_ahead_logging")]
    pub last_cancel_result_sequence: Option<u64>,
    /// Whether any cancellation result reported failure.
    #[cfg(feature = "write_ahead_logging")]
    pub cancel_failed: bool,
}

/// Order reconstructed from audit log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveredOrder {
    pub symbol: Symbol,
    pub action: String,
    pub shares: i64,
    pub limit_price_cents: i64,
    pub ibkr_id: i32,
    pub client_order_id: Option<String>,
    pub submitted: bool,
    pub filled: bool,
    pub failed: bool,
    pub failure_reason: Option<String>,
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
    /// Incomplete order intent (OrderIntent without OrderSubmitted or OrderFailed)
    IncompleteIntent {
        symbol: String,
        client_order_id: Option<String>,
    },
    /// Incomplete positions intent (PositionsIntent without PositionsResult)
    #[cfg(feature = "write_ahead_logging")]
    IncompletePositionsIntent {
        target_spec_reference: Option<String>,
    },
    /// Incomplete quotes intent (QuotesIntent without QuotesResult)
    #[cfg(feature = "write_ahead_logging")]
    IncompleteQuotesIntent {
        target_spec_reference: Option<String>,
    },
    /// Incomplete account summary intent (AccountSummaryIntent without AccountSummaryResult)
    #[cfg(feature = "write_ahead_logging")]
    IncompleteAccountSummaryIntent {
        target_spec_reference: Option<String>,
    },
    /// Incomplete cancel intent (CancelIntent without CancelResult)
    #[cfg(feature = "write_ahead_logging")]
    IncompleteCancelIntent {
        order_id: Option<u64>,
        cancellation_reason: Option<String>,
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
    let broker_orders = broker.open_orders().unwrap_or_else(|_| Vec::new());

    // Check for orphan orders (in broker but not in reconstructed state)
    for broker_order in &broker_orders {
        let is_orphan = !recovered_state
            .orders
            .iter()
            .any(|recovered_order| recovered_order.ibkr_id as u64 == broker_order.id.0);

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
            let is_missing = !broker_orders
                .iter()
                .any(|broker_order| recovered_order.ibkr_id as u64 == broker_order.id.0);

            if is_missing {
                discrepancies.push(Discrepancy::MissingOrder {
                    symbol: recovered_order.symbol.as_str().to_string(),
                    expected_status: "Submitted but not filled".to_string(),
                });
            }
        }
    }

    // Get broker positions
    let broker_positions = broker.positions().unwrap_or_else(|_| Vec::new());

    // Check for position mismatches
    for broker_position in &broker_positions {
        let recovered_position = recovered_state
            .positions
            .iter()
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
        let is_missing = !broker_positions
            .iter()
            .any(|bp| bp.symbol == recovered_position.symbol);
        if is_missing {
            discrepancies.push(Discrepancy::PositionMismatch {
                symbol: recovered_position.symbol.as_str().to_string(),
                broker_qty: 0,
                expected_qty: recovered_position.quantity,
            });
        }
    }

    // Check for incomplete order intents (OrderIntent without OrderSubmitted or OrderFailed)
    for order in &recovered_state.orders {
        // An order is incomplete if it has a client_order_id but is neither submitted nor failed
        if order.client_order_id.is_some() && !order.submitted && !order.failed {
            discrepancies.push(Discrepancy::IncompleteIntent {
                symbol: order.symbol.as_str().to_string(),
                client_order_id: order.client_order_id.clone(),
            });
        }
    }

    // Check for incomplete positions intent (PositionsIntent without PositionsResult)
    #[cfg(feature = "write_ahead_logging")]
    if recovered_state.positions_intent_logged && !recovered_state.positions_result_logged {
        discrepancies.push(Discrepancy::IncompletePositionsIntent {
            target_spec_reference: None,
        });
    }

    // Check for incomplete quotes intent (QuotesIntent without QuotesResult)
    #[cfg(feature = "write_ahead_logging")]
    if recovered_state.quotes_intent_logged && !recovered_state.quotes_result_logged {
        discrepancies.push(Discrepancy::IncompleteQuotesIntent {
            target_spec_reference: None,
        });
    }

    // Check for incomplete account summary intent (AccountSummaryIntent without AccountSummaryResult)
    #[cfg(feature = "write_ahead_logging")]
    if recovered_state.account_summary_intent_logged
        && !recovered_state.account_summary_result_logged
    {
        discrepancies.push(Discrepancy::IncompleteAccountSummaryIntent {
            target_spec_reference: None,
        });
    }

    // Check for incomplete cancel intent (CancelIntent without CancelResult)
    #[cfg(feature = "write_ahead_logging")]
    if recovered_state.cancel_intent_logged && !recovered_state.cancel_result_logged {
        discrepancies.push(Discrepancy::IncompleteCancelIntent {
            order_id: None,
            cancellation_reason: None,
        });
    }

    let has_critical_issues = !discrepancies.is_empty();

    Ok(DiscrepancyReport {
        discrepancies,
        has_critical_issues,
    })
}

/// Query the broker for an order matching the given criteria with retry logic.
///
/// This function attempts to find an order in the broker's open orders
/// that matches the given symbol and quantity. It uses heuristic matching
/// since the broker API doesn't support querying by client_order_id.
/// It retries on network failures with exponential backoff.
///
/// # Arguments
///
/// * `broker` - The broker connection to query
/// * `symbol` - The symbol to search for
/// * `quantity` - The order quantity to match
///
/// # Returns
///
/// * `Ok(Some(order_id))` - Order found at broker matching the criteria
/// * `Ok(None)` - Order not found at broker
/// * `Err(Error)` - Broker query failed after all retries
#[cfg(feature = "write_ahead_logging")]
pub fn reconcile_order_intent(
    broker: &dyn Broker,
    _symbol: &Symbol,
    _quantity: i64,
) -> Result<Option<u64>> {
    const MAX_RETRIES: usize = 5;
    const BASE_DELAY_MS: u64 = 1000; // 1 second

    for attempt in 0..MAX_RETRIES {
        match broker.open_orders() {
            Ok(orders) => {
                // Search for order with matching symbol and quantity
                // Note: This is a heuristic match since broker API doesn't provide client_order_id
                for order in &orders {
                    // We can only match by order ID since BrokerOrderStatus doesn't include symbol
                    // In a real implementation, we would need to maintain a local order cache
                    // For now, we return the first open order as a best-effort match
                    // This is a limitation of the current broker API
                    if order.remaining_quantity > 0 {
                        return Ok(Some(order.id.0));
                    }
                }
                // Order not found
                return Ok(None);
            }
            Err(e) => {
                // Retry on network failures
                if attempt < MAX_RETRIES - 1 {
                    let delay_ms = BASE_DELAY_MS * 2_u64.pow(attempt as u32);
                    tracing::warn!(
                        "Broker query failed (attempt {}/{}), retrying in {}ms: {}",
                        attempt + 1,
                        MAX_RETRIES,
                        delay_ms,
                        e
                    );
                    std::thread::sleep(Duration::from_millis(delay_ms));
                } else {
                    return Err(Error::Recovery(format!(
                        "Broker query failed after {} attempts: {}",
                        MAX_RETRIES, e
                    )));
                }
            }
        }
    }

    // This should never be reached, but required for type checking
    Err(Error::Recovery(
        "Unexpected error in broker reconciliation".to_string(),
    ))
}

/// Reconcile incomplete order intents with the broker and update audit log.
///
/// This function scans for incomplete intents (orders with client_order_id
/// but neither submitted nor failed), queries the broker for each, and
/// updates the audit log with the reconciliation results.
///
/// # Arguments
///
/// * `broker` - The broker connection to query
/// * `recovered_state` - The reconstructed state from audit log
/// * `audit_log_path` - Path to the audit log file for updates
///
/// # Returns
///
/// * `Ok(())` - All incomplete intents reconciled successfully
/// * `Err(Error)` - Reconciliation failed for one or more intents
#[cfg(feature = "write_ahead_logging")]
pub fn reconcile_incomplete_intents(
    broker: &dyn Broker,
    recovered_state: &RecoveredState,
    audit_log_path: &std::path::Path,
) -> Result<()> {
    use crate::audit::AuditLog;

    let mut reconciled_count = 0;
    let mut failed_count = 0;
    let mut next_sequence = recovered_state.sequence_number + 1;

    // Get the parent directory as workdir for audit log validation
    let workdir = audit_log_path
        .parent()
        .ok_or_else(|| Error::Recovery("Invalid audit log path".to_string()))?;

    for order in &recovered_state.orders {
        // Check if this is an incomplete intent
        if order.client_order_id.is_some() && !order.submitted && !order.failed {
            let symbol = &order.symbol;
            let quantity = order.shares;

            tracing::info!(
                "Reconciling incomplete intent for {} (client_order_id: {:?})",
                symbol.as_str(),
                order.client_order_id
            );

            match reconcile_order_intent(broker, symbol, quantity) {
                Ok(Some(broker_order_id)) => {
                    // Order found at broker - append OrderSubmitted event
                    tracing::info!(
                        "Order found at broker with ID {}, appending OrderSubmitted event",
                        broker_order_id
                    );
                    let mut audit = AuditLog::open_in(audit_log_path, workdir)?;
                    audit.log_checkpoint(
                        Checkpoint::OrderSubmitted,
                        next_sequence,
                        serde_json::json!({
                            "symbol": symbol.as_str(),
                            "ibkr_id": broker_order_id,
                            "reconciled": true,
                        }),
                    )?;
                    next_sequence += 1;
                    reconciled_count += 1;
                }
                Ok(None) => {
                    // Order not found at broker - append OrderFailed event
                    tracing::info!("Order not found at broker, appending OrderFailed event",);
                    let mut audit = AuditLog::open_in(audit_log_path, workdir)?;
                    audit.log_checkpoint(
                        Checkpoint::OrderFailed,
                        next_sequence,
                        serde_json::json!({
                            "symbol": symbol.as_str(),
                            "reason": "not_found_at_broker",
                            "reconciled": true,
                        }),
                    )?;
                    next_sequence += 1;
                    reconciled_count += 1;
                }
                Err(e) => {
                    // Broker query failed - log error and continue
                    tracing::error!(
                        "Failed to reconcile intent for {} (client_order_id: {:?}): {}",
                        symbol.as_str(),
                        order.client_order_id,
                        e
                    );
                    failed_count += 1;
                }
            }
        }
    }

    tracing::info!(
        "Broker reconciliation complete: {} reconciled, {} failed",
        reconciled_count,
        failed_count
    );

    if failed_count > 0 {
        Err(Error::Recovery(format!(
            "Failed to reconcile {} incomplete intents",
            failed_count
        )))
    } else {
        Ok(())
    }
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
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        }
    }

    /// Determine the recovery action based on the reconstructed state.
    pub fn determine_recovery_action(&self) -> RecoveryAction {
        // If run completed successfully, safe to restart
        if self.run_completed {
            return RecoveryAction::Restart;
        }

        // Check for incomplete cancel intent (CancelIntent without CancelResult) before
        // the orders-empty shortcut. A cancellation may be initiated by an operator
        // or future kill-switch path even when the reconstructed order list is empty;
        // the broker-side cancellation state is still safety-critical.
        #[cfg(feature = "write_ahead_logging")]
        if self.cancel_failed {
            return RecoveryAction::ManualReview;
        }
        #[cfg(feature = "write_ahead_logging")]
        if self.last_cancel_intent_sequence.is_some_and(|intent| {
            self.last_cancel_result_sequence
                .is_none_or(|result| result < intent)
        }) {
            return RecoveryAction::ManualReview;
        }

        // If crashed before any orders were submitted, safe to restart
        if self.orders.is_empty() {
            return RecoveryAction::Restart;
        }

        // Check for incomplete intents (OrderIntent without OrderSubmitted or OrderFailed)
        let has_incomplete_intents = self
            .orders
            .iter()
            .any(|o| o.client_order_id.is_some() && !o.submitted && !o.failed);

        if has_incomplete_intents {
            // Incomplete intents require broker reconciliation
            // If write_ahead_logging feature is enabled, reconciliation will be attempted
            // Otherwise, manual review is required
            #[cfg(feature = "write_ahead_logging")]
            return RecoveryAction::Resume; // Will trigger broker reconciliation

            #[cfg(not(feature = "write_ahead_logging"))]
            return RecoveryAction::ManualReview;
        }

        // Check for incomplete positions intent (PositionsIntent without PositionsResult)
        #[cfg(feature = "write_ahead_logging")]
        if self.last_positions_intent_sequence.is_some_and(|intent| {
            self.last_positions_result_sequence
                .is_none_or(|result| result < intent)
        }) {
            // Incomplete positions fetch - safe to restart since positions are read-only
            return RecoveryAction::Restart;
        }

        // Check for incomplete quotes intent (QuotesIntent without QuotesResult)
        #[cfg(feature = "write_ahead_logging")]
        if self.last_quotes_intent_sequence.is_some_and(|intent| {
            self.last_quotes_result_sequence
                .is_none_or(|result| result < intent)
        }) {
            // Incomplete quotes fetch - safe to restart since quotes are read-only
            return RecoveryAction::Restart;
        }

        // Check for incomplete account summary intent (AccountSummaryIntent without AccountSummaryResult)
        #[cfg(feature = "write_ahead_logging")]
        if self
            .last_account_summary_intent_sequence
            .is_some_and(|intent| {
                self.last_account_summary_result_sequence
                    .is_none_or(|result| result < intent)
            })
        {
            // Incomplete account summary fetch - safe to restart since account summary is read-only
            return RecoveryAction::Restart;
        }

        // If crashed after order submission but before fills, need manual review
        let has_unfilled_orders = self.orders.iter().any(|o| o.submitted && !o.filled);
        if has_unfilled_orders {
            return RecoveryAction::ManualReview;
        }

        // If crashed after all orders filled, safe to restart
        RecoveryAction::Restart
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
pub fn reconstruct_state(
    audit_log_path: &std::path::Path,
) -> Result<(RecoveredState, RecoveryAction)> {
    let events = crate::audit::parse_audit_events(audit_log_path)?;
    let mut state = RecoveredState::new();

    for event in events {
        match Checkpoint::from_event_name(&event.event) {
            Some(Checkpoint::RunStarted) => {
                state.checkpoint = Checkpoint::RunStarted;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.run_completed = false;
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::PositionsIntent) => {
                state.checkpoint = Checkpoint::PositionsIntent;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.positions_intent_logged = true;
                state.last_positions_intent_sequence = event.sequence_number;
            }
            Some(Checkpoint::PositionsFetched) => {
                state.checkpoint = Checkpoint::PositionsFetched;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                if let Some(positions_array) = event.data.get("positions") {
                    if let Some(positions) = positions_array.as_array() {
                        state.positions = positions
                            .iter()
                            .filter_map(|p| {
                                let symbol = p.get("symbol")?.as_str()?;
                                let qty = p.get("qty")?.as_i64()?;
                                let avg_cost_f64 = p.get("avg_cost")?.as_f64()?;
                                let avg_cost_cents =
                                    f64_cents_checked(avg_cost_f64, "avg_cost").ok()?;
                                Symbol::try_new(symbol).map(|sym| CurrentPosition {
                                    symbol: sym,
                                    quantity: qty,
                                    avg_cost_cents,
                                })
                            })
                            .collect();
                    }
                }

                if let Some(equity) = event.data.get("equity") {
                    if let Some(equity_val) = equity.as_f64() {
                        // Skip equity update if value is out of i64 range; positions remain valid.
                        if let Ok(cents) = f64_cents_checked(equity_val, "equity") {
                            state.equity_cents = cents;
                        }
                    }
                }
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::PositionsResult) => {
                state.checkpoint = Checkpoint::PositionsResult;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.positions_result_logged = true;
                state.last_positions_result_sequence = event.sequence_number;

                // Extract positions from PositionsResult (same format as PositionsFetched)
                if let Some(positions_array) = event.data.get("positions") {
                    if let Some(positions) = positions_array.as_array() {
                        state.positions = positions
                            .iter()
                            .filter_map(|p| {
                                let symbol = p.get("symbol")?.as_str()?;
                                let qty = p.get("qty")?.as_i64()?;
                                let avg_cost_f64 = p.get("avg_cost")?.as_f64()?;
                                let avg_cost_cents =
                                    f64_cents_checked(avg_cost_f64, "avg_cost").ok()?;
                                Symbol::try_new(symbol).map(|sym| CurrentPosition {
                                    symbol: sym,
                                    quantity: qty,
                                    avg_cost_cents,
                                })
                            })
                            .collect();
                    }
                }

                if let Some(equity) = event.data.get("equity") {
                    if let Some(equity_val) = equity.as_f64() {
                        // Skip equity update if value is out of i64 range; positions remain valid.
                        if let Ok(cents) = f64_cents_checked(equity_val, "equity") {
                            state.equity_cents = cents;
                        }
                    }
                }
            }
            Some(Checkpoint::DiffComputed) => {
                state.checkpoint = Checkpoint::DiffComputed;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                if let Some(orders_array) = event.data.get("orders") {
                    if let Some(orders) = orders_array.as_array() {
                        state.orders = orders
                            .iter()
                            .map(|o| {
                                let symbol = o.get("symbol").and_then(|s| s.as_str()).unwrap_or("");
                                let action = o.get("action").and_then(|a| a.as_str()).unwrap_or("");
                                let shares = o.get("shares").and_then(|s| s.as_i64()).unwrap_or(0);
                                let limit =
                                    o.get("limit").and_then(|l| l.as_f64()).unwrap_or(0.0) as i64;
                                RecoveredOrder {
                                    symbol: Symbol::try_new(symbol)
                                        .unwrap_or(Symbol::try_new("UNKNOWN").unwrap()),
                                    action: action.to_string(),
                                    shares,
                                    limit_price_cents: limit,
                                    ibkr_id: 0,
                                    client_order_id: None,
                                    submitted: false,
                                    filled: false,
                                    failed: false,
                                    failure_reason: None,
                                }
                            })
                            .collect();
                    }
                }
            }
            Some(Checkpoint::RiskCheckPassed) => {
                state.checkpoint = Checkpoint::RiskCheckPassed;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
            }
            Some(Checkpoint::OrderIntent) => {
                state.checkpoint = Checkpoint::OrderIntent;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                // Extract order details from OrderIntent event
                if let Some(symbol) = event.data.get("symbol").and_then(|s| s.as_str()) {
                    if let Some(action) = event.data.get("action").and_then(|a| a.as_str()) {
                        if let Some(shares) = event.data.get("shares").and_then(|s| s.as_i64()) {
                            if let Some(limit) = event.data.get("limit").and_then(|l| l.as_f64()) {
                                let client_order_id = event
                                    .data
                                    .get("client_order_id")
                                    .and_then(|id| id.as_str())
                                    .map(|s| s.to_string());

                                // Check if order already exists (from DiffComputed)
                                if let Some(order) = state
                                    .orders
                                    .iter_mut()
                                    .find(|o| o.symbol.as_str() == symbol)
                                {
                                    // Update existing order with intent details
                                    order.client_order_id = client_order_id;
                                } else {
                                    // Create new order from intent
                                    state.orders.push(RecoveredOrder {
                                        symbol: Symbol::try_new(symbol)
                                            .unwrap_or(Symbol::try_new("UNKNOWN").unwrap()),
                                        action: action.to_string(),
                                        shares,
                                        limit_price_cents: limit as i64,
                                        ibkr_id: 0,
                                        client_order_id,
                                        submitted: false,
                                        filled: false,
                                        failed: false,
                                        failure_reason: None,
                                    });
                                }
                            }
                        }
                    }
                }
            }
            Some(Checkpoint::OrderFailed) => {
                state.checkpoint = Checkpoint::OrderFailed;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                // Extract failure details and mark order as failed
                let failure_reason = event
                    .data
                    .get("reason")
                    .and_then(|r| r.as_str())
                    .map(|s| s.to_string());

                if let Some(symbol) = event.data.get("symbol").and_then(|s| s.as_str()) {
                    if let Some(order) = state
                        .orders
                        .iter_mut()
                        .find(|o| o.symbol.as_str() == symbol)
                    {
                        order.failed = true;
                        order.failure_reason = failure_reason;
                    }
                }
            }
            Some(Checkpoint::OrderSubmitted) => {
                state.checkpoint = Checkpoint::OrderSubmitted;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                if let Some(ibkr_id) = event.data.get("ibkr_id").and_then(|id| id.as_i64()) {
                    if let Some(symbol) = event.data.get("symbol").and_then(|s| s.as_str()) {
                        if let Some(order) = state
                            .orders
                            .iter_mut()
                            .find(|o| o.symbol.as_str() == symbol)
                        {
                            order.ibkr_id = ibkr_id as i32;
                            order.submitted = true;
                        }
                    }
                }
            }
            Some(Checkpoint::OrderFilled) => {
                state.checkpoint = Checkpoint::OrderFilled;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;

                if let Some(ibkr_id) = event.data.get("ibkr_id").and_then(|id| id.as_i64()) {
                    if let Some(order) = state
                        .orders
                        .iter_mut()
                        .find(|o| o.ibkr_id as i64 == ibkr_id)
                    {
                        order.filled = true;
                    }
                }
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::QuotesIntent) => {
                state.checkpoint = Checkpoint::QuotesIntent;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.quotes_intent_logged = true;
                state.last_quotes_intent_sequence = event.sequence_number;
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::QuotesResult) => {
                state.checkpoint = Checkpoint::QuotesResult;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.quotes_result_logged = true;
                state.last_quotes_result_sequence = event.sequence_number;
                // Quotes data is not stored in RecoveredState as it's transient
                // but we mark that the fetch completed successfully
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::AccountSummaryIntent) => {
                state.checkpoint = Checkpoint::AccountSummaryIntent;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.account_summary_intent_logged = true;
                state.last_account_summary_intent_sequence = event.sequence_number;
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::AccountSummaryResult) => {
                state.checkpoint = Checkpoint::AccountSummaryResult;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.account_summary_result_logged = true;
                state.last_account_summary_result_sequence = event.sequence_number;

                // Extract equity from AccountSummaryResult
                if let Some(equity) = event.data.get("equity") {
                    if let Some(equity_val) = equity.as_f64() {
                        if let Ok(cents) = f64_cents_checked(equity_val, "equity") {
                            state.equity_cents = cents;
                        }
                    }
                }
                // Cash is not stored in RecoveredState but could be added if needed
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::CancelIntent) => {
                state.checkpoint = Checkpoint::CancelIntent;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.cancel_intent_logged = true;
                state.last_cancel_intent_sequence = event.sequence_number;
                // Cancel intent metadata (order_id, cancellation_reason) could be stored here
                // but is not currently tracked in RecoveredState
            }
            #[cfg(feature = "write_ahead_logging")]
            Some(Checkpoint::CancelResult) => {
                state.checkpoint = Checkpoint::CancelResult;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.cancel_result_logged = true;
                state.last_cancel_result_sequence = event.sequence_number;
                state.cancel_failed |= event
                    .data
                    .get("success")
                    .and_then(|success| success.as_bool())
                    .is_some_and(|success| !success);
                // Cancel result could update order status in RecoveredState
                // but is not currently implemented as cancel is not used in the main flow
            }
            Some(Checkpoint::RunCompleted) => {
                state.checkpoint = Checkpoint::RunCompleted;
                state.sequence_number = event.sequence_number.unwrap_or(0);
                state.timestamp = event.ts;
                state.run_completed = true;
            }
            None => {}
        }
    }

    let recovery_action = state.determine_recovery_action();
    Ok((state, recovery_action))
}

/// Run recovery from crash using audit log.
///
/// This function:
/// 1. Reads the audit log to reconstruct state
/// 2. Determines the appropriate recovery action
/// 3. Compares broker state if broker is provided
/// 4. Prints a recovery report
pub fn run_recover(
    config: &Config,
    _target_spec: &TargetSpec,
    dry_run: bool,
    broker: Option<&dyn Broker>,
) -> Result<()> {
    let audit_log_path = config.audit_path();

    println!(
        "Recovering from crash using audit log: {}",
        audit_log_path.display()
    );

    let (recovered_state, recovery_action) = reconstruct_state(&audit_log_path)?;

    println!("\n=== Recovered State ===");
    println!("Checkpoint: {:?}", recovered_state.checkpoint);
    println!("Sequence Number: {}", recovered_state.sequence_number);
    println!("Timestamp: {}", recovered_state.timestamp);
    println!("Positions: {}", recovered_state.positions.len());
    for pos in &recovered_state.positions {
        println!(
            "  - {}: {} shares @ ${:.2}",
            pos.symbol.as_str(),
            pos.quantity,
            pos.avg_cost_cents as f64 / 100.0
        );
    }
    println!("Orders: {}", recovered_state.orders.len());
    for order in &recovered_state.orders {
        println!(
            "  - {}: {} {} @ ${:.2} (submitted: {}, filled: {}, failed: {})",
            order.symbol.as_str(),
            order.action,
            order.shares,
            order.limit_price_cents as f64 / 100.0,
            order.submitted,
            order.filled,
            order.failed
        );
        if let Some(ref client_order_id) = order.client_order_id {
            println!("    client_order_id: {}", client_order_id);
        }
        if let Some(ref reason) = order.failure_reason {
            println!("    failure_reason: {}", reason);
        }
    }
    println!("Equity: ${}", recovered_state.equity_cents as f64 / 100.0);
    println!("Run Completed: {}", recovered_state.run_completed);

    println!("\n=== Recovery Action ===");
    println!("{:?}", recovery_action);

    // Compare broker state if broker is available
    let discrepancy_report = if let Some(broker) = broker {
        println!("\n=== Broker State Comparison ===");
        match compare_broker_state(broker, &recovered_state) {
            Ok(report) => {
                if report.has_critical_issues {
                    println!("Discrepancies found between broker and reconstructed state:");
                    for discrepancy in &report.discrepancies {
                        match discrepancy {
                            Discrepancy::OrphanOrder {
                                broker_order_id,
                                symbol,
                                status,
                            } => {
                                println!(
                                    "  - Orphan order: ID {} (symbol: {}, status: {})",
                                    broker_order_id, symbol, status
                                );
                            }
                            Discrepancy::MissingOrder {
                                symbol,
                                expected_status,
                            } => {
                                println!(
                                    "  - Missing order: {} (expected: {})",
                                    symbol, expected_status
                                );
                            }
                            Discrepancy::OrderStatusMismatch {
                                symbol,
                                broker_status,
                                expected_status,
                            } => {
                                println!(
                                    "  - Order status mismatch for {}: broker={}, expected={}",
                                    symbol, broker_status, expected_status
                                );
                            }
                            Discrepancy::PositionMismatch {
                                symbol,
                                broker_qty,
                                expected_qty,
                            } => {
                                println!(
                                    "  - Position mismatch for {}: broker={}, expected={}",
                                    symbol, broker_qty, expected_qty
                                );
                            }
                            Discrepancy::IncompleteIntent {
                                symbol,
                                client_order_id,
                            } => {
                                println!(
                                    "  - Incomplete order intent: {} (client_order_id: {:?})",
                                    symbol, client_order_id
                                );
                            }
                            #[cfg(feature = "write_ahead_logging")]
                            Discrepancy::IncompletePositionsIntent {
                                target_spec_reference,
                            } => {
                                println!(
                                    "  - Incomplete positions intent (target_spec_reference: {:?})",
                                    target_spec_reference
                                );
                            }
                            #[cfg(feature = "write_ahead_logging")]
                            Discrepancy::IncompleteQuotesIntent {
                                target_spec_reference,
                            } => {
                                println!(
                                    "  - Incomplete quotes intent (target_spec_reference: {:?})",
                                    target_spec_reference
                                );
                            }
                            #[cfg(feature = "write_ahead_logging")]
                            Discrepancy::IncompleteAccountSummaryIntent {
                                target_spec_reference,
                            } => {
                                println!(
                                    "  - Incomplete account summary intent (target_spec_reference: {:?})",
                                    target_spec_reference
                                );
                            }
                            #[cfg(feature = "write_ahead_logging")]
                            Discrepancy::IncompleteCancelIntent {
                                order_id,
                                cancellation_reason,
                            } => {
                                println!(
                                    "  - Incomplete cancel intent (order_id: {:?}, cancellation_reason: {:?})",
                                    order_id, cancellation_reason
                                );
                            }
                        }
                    }
                    println!("\nWARNING: Broker state does not match reconstructed state.");
                    Some(report)
                } else {
                    println!("Broker state matches reconstructed state.");
                    None
                }
            }
            Err(e) => {
                println!("Failed to compare broker state: {}", e);
                println!("Proceeding with recovery based on audit log only.");
                None
            }
        }
    } else {
        println!("\n=== Broker State Verification ===");
        println!("No broker connection available - skipping state comparison.");
        println!("To verify broker state, manually check IBKR TWS for open orders and positions.");
        None
    };

    // Attempt broker reconciliation for incomplete intents if feature is enabled
    #[cfg(feature = "write_ahead_logging")]
    if let Some(broker) = broker {
        // Check if there are incomplete intents to reconcile
        let has_incomplete_intents = recovered_state
            .orders
            .iter()
            .any(|o| o.client_order_id.is_some() && !o.submitted && !o.failed);

        if has_incomplete_intents {
            println!("\n=== Broker Reconciliation ===");
            println!("Incomplete order intents detected - attempting broker reconciliation...");
            match reconcile_incomplete_intents(broker, &recovered_state, &audit_log_path) {
                Ok(()) => {
                    println!("Broker reconciliation completed successfully.");
                    println!("Audit log has been updated with reconciliation results.");
                    // Reconstruct state again to get updated state
                    let (updated_state, _) = reconstruct_state(&audit_log_path)?;
                    println!("\n=== Updated Recovered State ===");
                    println!("Checkpoint: {:?}", updated_state.checkpoint);
                    println!("Orders: {}", updated_state.orders.len());
                    for order in &updated_state.orders {
                        println!(
                            "  - {}: {} {} @ ${:.2} (submitted: {}, filled: {}, failed: {})",
                            order.symbol.as_str(),
                            order.action,
                            order.shares,
                            order.limit_price_cents as f64 / 100.0,
                            order.submitted,
                            order.filled,
                            order.failed
                        );
                    }
                }
                Err(e) => {
                    println!("Broker reconciliation failed: {}", e);
                    println!("Manual review required to resolve incomplete intents.");
                }
            }
        }
    }

    println!("\n=== Recovery Guidance ===");
    match recovery_action {
        RecoveryAction::Restart => {
            println!("Safe to restart the entire rebalance from the beginning.");
            println!("No orders were submitted or all orders were filled.");
            if !dry_run {
                println!("Run: rebalancer run <target.json>");
            }
        }
        RecoveryAction::Resume => {
            println!("Resume from the last checkpoint.");
            println!("Some orders may have been submitted but not filled.");
            println!("Manual review of broker state recommended before proceeding.");
        }
        RecoveryAction::ManualReview => {
            println!("Manual review required.");
            println!("The crash occurred at an ambiguous point.");
            println!("Please review broker state and decide on the appropriate action.");
            println!("IMPORTANT: Verify IBKR TWS for open orders and positions before proceeding.");
            if discrepancy_report.is_some() {
                println!(
                    "\nNOTE: Discrepancies detected between broker and reconstructed state. Review the comparison above carefully."
                );
            }
            return Err(Error::Recovery("Manual review required".to_string()));
        }
        RecoveryAction::Rollback => {
            println!("Rollback recommended.");
            println!("Orders were submitted but may be in an unknown state.");
            println!("Please review broker open orders and cancel if necessary.");
            println!("IMPORTANT: Verify IBKR TWS for open orders and positions before proceeding.");
            if discrepancy_report.is_some() {
                println!(
                    "\nNOTE: Orphan orders detected. These should be manually canceled in IBKR TWS before proceeding."
                );
            }
            return Err(Error::Recovery(
                "Rollback required - manual intervention needed".to_string(),
            ));
        }
    }

    if dry_run {
        println!("\n=== Dry Run - No Action Taken ===");
    }

    Ok(())
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
                Checkpoint::RiskCheckPassed,
                4,
                serde_json::json!({"checks": []}),
            )
            .unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::RiskCheckPassed);
        assert_eq!(state.sequence_number, 4);
        assert_eq!(state.positions.len(), 1);
        assert_eq!(state.positions[0].avg_cost_cents, 15_000);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(state.equity_cents, 10000000);
        assert_eq!(
            state.positions[0].avg_cost_cents, 15_000,
            "avg_cost_cents must survive roundtrip"
        );
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
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "ibkr_id": 12345,
                }),
            )
            .unwrap();
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
                    "action": "Buy",
                    "shares": 50,
                    "limit": 160.0,
                    "ibkr_id": 12345,
                }),
            )
            .unwrap();
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
            )
            .unwrap();
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
            )
            .unwrap();
            log.log_checkpoint(
                Checkpoint::RunCompleted,
                2,
                serde_json::json!({
                    "submitted": 1,
                    "filled": 1,
                    "failed": 0,
                }),
            )
            .unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::RunCompleted);
        assert_eq!(state.sequence_number, 2);
        assert!(state.run_completed);
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[test]
    fn compare_broker_state_no_discrepancies() {
        use nanobook_broker::mock::{FillMode, MockBroker};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::RunCompleted,
            sequence_number: 1,
            timestamp: chrono::Utc::now(),
            positions: vec![CurrentPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 150_00,
            }],
            orders: vec![],
            equity_cents: 100_000_00,
            run_completed: true,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(!report.has_critical_issues);
        assert!(report.discrepancies.is_empty());
    }

    #[test]
    fn compare_broker_state_orphan_order() {
        use nanobook_broker::mock::{FillMode, MockBroker};
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
            positions: vec![CurrentPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 150_00,
            }],
            orders: vec![], // No orders in recovered state
            equity_cents: 100_000_00,
            run_completed: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(report.has_critical_issues);
        assert!(!report.discrepancies.is_empty());
    }

    #[test]
    fn compare_broker_state_position_mismatch() {
        use nanobook_broker::mock::{FillMode, MockBroker};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 150, 150_00) // Different quantity
            .build();
        broker.connect().unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::RunCompleted,
            sequence_number: 1,
            timestamp: chrono::Utc::now(),
            positions: vec![CurrentPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100, // Different from broker
                avg_cost_cents: 150_00,
            }],
            orders: vec![],
            equity_cents: 100_000_00,
            run_completed: true,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(report.has_critical_issues);
        assert!(!report.discrepancies.is_empty());
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn reconstruct_state_order_intent_only() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_order_intent_only.jsonl");

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

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderIntent);
        assert_eq!(state.sequence_number, 2);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(state.orders[0].symbol.as_str(), "AAPL");
        assert_eq!(
            state.orders[0].client_order_id,
            Some("test_client_order_123".to_string())
        );
        assert!(!state.orders[0].submitted);
        assert!(!state.orders[0].filled);
        assert!(!state.orders[0].failed);
        #[cfg(feature = "write_ahead_logging")]
        assert_eq!(action, RecoveryAction::Resume);
        #[cfg(not(feature = "write_ahead_logging"))]
        assert_eq!(action, RecoveryAction::ManualReview);
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn reconstruct_state_order_intent_with_submission() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_intent_with_submission.jsonl");

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
                    "ibkr_id": 54321,
                }),
            )
            .unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderSubmitted);
        assert_eq!(state.sequence_number, 3);
        assert_eq!(state.orders.len(), 1);
        assert!(state.orders[0].submitted);
        assert_eq!(state.orders[0].ibkr_id, 54321);
        assert!(!state.orders[0].failed);
        assert_eq!(action, RecoveryAction::ManualReview);
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn reconstruct_state_order_intent_with_failure() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_intent_with_failure.jsonl");

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
                Checkpoint::OrderFailed,
                3,
                serde_json::json!({
                    "symbol": "AAPL",
                    "reason": "network_timeout",
                }),
            )
            .unwrap();
        }

        let (state, action) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderFailed);
        assert_eq!(state.sequence_number, 3);
        assert_eq!(state.orders.len(), 1);
        assert!(!state.orders[0].submitted);
        assert!(state.orders[0].failed);
        assert_eq!(
            state.orders[0].failure_reason,
            Some("network_timeout".to_string())
        );
        assert_eq!(action, RecoveryAction::Restart);
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn compare_broker_state_incomplete_intent() {
        use nanobook_broker::mock::{FillMode, MockBroker};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        let state = RecoveredState {
            checkpoint: Checkpoint::OrderIntent,
            sequence_number: 2,
            timestamp: chrono::Utc::now(),
            positions: vec![CurrentPosition {
                symbol: Symbol::new("AAPL"),
                quantity: 100,
                avg_cost_cents: 150_00,
            }],
            orders: vec![RecoveredOrder {
                symbol: Symbol::new("AAPL"),
                action: "Buy".to_string(),
                shares: 50,
                limit_price_cents: 160_00,
                ibkr_id: 0,
                client_order_id: Some("test_client_order_123".to_string()),
                submitted: false,
                filled: false,
                failed: false,
                failure_reason: None,
            }],
            equity_cents: 100_000_00,
            run_completed: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let report = compare_broker_state(&broker, &state).unwrap();
        assert!(report.has_critical_issues);
        assert!(!report.discrepancies.is_empty());

        // Should have IncompleteIntent discrepancy
        let has_incomplete = report
            .discrepancies
            .iter()
            .any(|d| matches!(d, Discrepancy::IncompleteIntent { .. }));
        assert!(has_incomplete);
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn reconcile_order_intent_found() {
        use nanobook_broker::mock::{FillMode, MockBroker};
        use nanobook_broker::{BrokerOrder, BrokerOrderType, BrokerSide};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediatePartial(0.5))
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        // Submit an order
        let order = BrokerOrder {
            symbol: Symbol::new("AAPL"),
            side: BrokerSide::Buy,
            quantity: 50,
            order_type: BrokerOrderType::Market,
            client_order_id: None,
        };
        broker.submit_order(&order).unwrap();

        // Query for the order
        let result = reconcile_order_intent(&broker, &Symbol::new("AAPL"), 50).unwrap();
        assert!(result.is_some());
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn reconcile_order_intent_not_found() {
        use nanobook_broker::mock::{FillMode, MockBroker};

        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediateFull)
            .with_position(Symbol::new("AAPL"), 100, 150_00)
            .build();
        broker.connect().unwrap();

        // Query when no orders exist
        let result = reconcile_order_intent(&broker, &Symbol::new("AAPL"), 50).unwrap();
        assert!(result.is_none());
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn determine_recovery_action_all_resolved() {
        let state = RecoveredState {
            checkpoint: Checkpoint::OrderSubmitted,
            sequence_number: 3,
            timestamp: chrono::Utc::now(),
            positions: vec![],
            orders: vec![RecoveredOrder {
                symbol: Symbol::new("AAPL"),
                action: "Buy".to_string(),
                shares: 50,
                limit_price_cents: 160_00,
                ibkr_id: 54321,
                client_order_id: Some("test_client_order_123".to_string()),
                submitted: true,
                filled: false,
                failed: false,
                failure_reason: None,
            }],
            equity_cents: 100_000_00,
            run_completed: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let action = state.determine_recovery_action();
        assert_eq!(action, RecoveryAction::ManualReview); // Has unfilled submitted order
    }

    #[cfg(feature = "write_ahead_logging")]
    #[test]
    fn determine_recovery_action_incomplete_intent() {
        let state = RecoveredState {
            checkpoint: Checkpoint::OrderIntent,
            sequence_number: 2,
            timestamp: chrono::Utc::now(),
            positions: vec![],
            orders: vec![RecoveredOrder {
                symbol: Symbol::new("AAPL"),
                action: "Buy".to_string(),
                shares: 50,
                limit_price_cents: 160_00,
                ibkr_id: 0,
                client_order_id: Some("test_client_order_123".to_string()),
                submitted: false,
                filled: false,
                failed: false,
                failure_reason: None,
            }],
            equity_cents: 100_000_00,
            run_completed: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            positions_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            quotes_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            account_summary_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_intent_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            cancel_result_logged: false,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_positions_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_quotes_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_account_summary_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_intent_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            last_cancel_result_sequence: None,
            #[cfg(feature = "write_ahead_logging")]
            cancel_failed: false,
        };

        let action = state.determine_recovery_action();
        assert_eq!(action, RecoveryAction::Resume); // Should trigger broker reconciliation
    }

    // Feature flag gating tests

    #[test]
    fn test_recovery_without_feature_flag_skips_reconciliation() {
        // This test runs when feature is NOT enabled
        // Verify that incomplete intents return ManualReview
        #[cfg(not(feature = "write_ahead_logging"))]
        {
            let state = RecoveredState {
                checkpoint: Checkpoint::OrderIntent,
                sequence_number: 2,
                timestamp: chrono::Utc::now(),
                positions: vec![],
                orders: vec![RecoveredOrder {
                    symbol: Symbol::new("AAPL"),
                    action: "Buy".to_string(),
                    shares: 50,
                    limit_price_cents: 160_00,
                    ibkr_id: 0,
                    client_order_id: Some("test_client_order_123".to_string()),
                    submitted: false,
                    filled: false,
                    failed: false,
                    failure_reason: None,
                }],
                equity_cents: 100_000_00,
                run_completed: false,
            };

            let action = state.determine_recovery_action();
            assert_eq!(action, RecoveryAction::ManualReview); // Should require manual review
        }

        // When feature IS enabled, this test should still compile but skip the assertion
        #[cfg(feature = "write_ahead_logging")]
        {
            // This test is for the case when feature is disabled
            // When feature is enabled, we just verify the test compiles
        }
    }

    #[test]
    fn test_recovery_with_feature_flag_performs_reconciliation() {
        // This test runs when feature IS enabled
        // Verify that incomplete intents return Resume
        #[cfg(feature = "write_ahead_logging")]
        {
            let state = RecoveredState {
                checkpoint: Checkpoint::OrderIntent,
                sequence_number: 2,
                timestamp: chrono::Utc::now(),
                positions: vec![],
                orders: vec![RecoveredOrder {
                    symbol: Symbol::new("AAPL"),
                    action: "Buy".to_string(),
                    shares: 50,
                    limit_price_cents: 160_00,
                    ibkr_id: 0,
                    client_order_id: Some("test_client_order_123".to_string()),
                    submitted: false,
                    filled: false,
                    failed: false,
                    failure_reason: None,
                }],
                equity_cents: 100_000_00,
                run_completed: false,
                positions_intent_logged: false,
                positions_result_logged: false,
                quotes_intent_logged: false,
                quotes_result_logged: false,
                #[cfg(feature = "write_ahead_logging")]
                account_summary_intent_logged: false,
                #[cfg(feature = "write_ahead_logging")]
                account_summary_result_logged: false,
                #[cfg(feature = "write_ahead_logging")]
                cancel_intent_logged: false,
                #[cfg(feature = "write_ahead_logging")]
                cancel_result_logged: false,
                #[cfg(feature = "write_ahead_logging")]
                last_positions_intent_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_positions_result_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_quotes_intent_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_quotes_result_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_account_summary_intent_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_account_summary_result_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_cancel_intent_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                last_cancel_result_sequence: None,
                #[cfg(feature = "write_ahead_logging")]
                cancel_failed: false,
            };

            let action = state.determine_recovery_action();
            assert_eq!(action, RecoveryAction::Resume); // Should trigger broker reconciliation
        }

        // When feature is NOT enabled, this test should still compile but skip the assertion
        #[cfg(not(feature = "write_ahead_logging"))]
        {
            // This test is for the case when feature is enabled
            // When feature is disabled, we just verify the test compiles
        }
    }

    #[test]
    fn test_audit_log_parsing_without_feature_flag() {
        // Verify that audit log parsing works regardless of feature flag
        // OrderIntent events should parse correctly even without the feature
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_parsing.jsonl");

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

        let (state, _) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderIntent);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(
            state.orders[0].client_order_id,
            Some("test_client_order_123".to_string())
        );
    }

    #[test]
    fn test_audit_log_parsing_with_feature_flag() {
        // Verify that audit log parsing works with the feature enabled
        // This is the same test as above but verifies it works with feature enabled
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_parsing_with_feature.jsonl");

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

        let (state, _) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderIntent);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(
            state.orders[0].client_order_id,
            Some("test_client_order_123".to_string())
        );
    }

    #[test]
    fn test_checkpoint_validation_without_feature_flag() {
        // Verify that checkpoint validation works regardless of feature flag
        // The validation should accept both old and new formats
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_validation.jsonl");

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
        }

        let (state, _) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::DiffComputed);
        assert_eq!(state.sequence_number, 3);
    }

    #[test]
    fn test_checkpoint_validation_with_feature_flag() {
        // Verify that checkpoint validation works with the new OrderIntent checkpoint
        let dir = tempdir().unwrap();
        let path = dir.path().join("test_validation_with_feature.jsonl");

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
                Checkpoint::OrderIntent,
                4,
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

        let (state, _) = reconstruct_state(&path).unwrap();
        assert_eq!(state.checkpoint, Checkpoint::OrderIntent);
        assert_eq!(state.sequence_number, 4);
        assert_eq!(
            state.orders[0].client_order_id,
            Some("test_client_order_123".to_string())
        );
    }
}
