# Ops Hardening Learnings: v0.13 Failure Modes

## Overview

This document captures the learnings from implementing 9 IBKR failure modes in v0.13. Each failure mode represents a real-world operational scenario that could cause incorrect trading behavior, data corruption, or system instability. The goal was to identify gaps in v0.10 hardening and implement systematic defenses.

**Purpose:** Document what v0.13 failure modes surfaced and what changed compared to v0.10 hardening.

**Scope:** 9 IBKR failure modes (F1-F9) covering duplicate callbacks, race conditions, disconnects, stale data, clock skew, restarts, idempotency, kill switches, and crash recovery.

**Relationship to v0.10 hardening:** v0.10 focused on security hardening (rustls, credential zeroization), error handling improvements (Result types), validation (config sandboxing), and numerical correctness. v0.13 builds on this by adding operational resilience, state reconciliation, and failure detection.

## Failure Mode Summary Table

| Failure Mode | Description | v0.10 Handling | v0.13 Changes | New Bug? |
|-------------|-------------|----------------|---------------|----------|
| F1: Duplicate callbacks | TWS re-sends order-status callbacks | Partial (basic dedup) | Full dedup with cache | No |
| F2: Cancel reject race | Cancel races against in-flight fill | None | Reconciliation logic | Yes |
| F3: Partial fill + disconnect | Order partially fills, then TWS drops | None | Reconnect + position query | Yes |
| F4: Stale market data | Snapshot held while quotes move | None | Staleness detection | Yes |
| F5: Clock skew | NTP drift or VM clock jump | Partial (basic clock) | Full skew detection | No |
| F6: TWS restart | TWS restarts with open positions | None | Reconnect + reconciliation | Yes |
| F7: Cron double-fire | Cron misfires, runs twice | None | Sequence number check | Yes |
| F8: Kill switch | Emergency stop of running runner | None | --kill subcommand | Yes |
| F9: Process crash | SIGKILL mid-rebalance | None | Audit log recovery | Yes |

**Summary:** 7 of 9 failure modes surfaced new bugs or gaps (F2, F3, F4, F6, F7, F8, F9). 2 were enhancements to existing partial handling (F1, F5).

## Detailed Analysis per Failure Mode

### F1: Duplicate Order-Status Callbacks

**Problem:** TWS occasionally re-sends order-status callbacks. Without deduplication, the broker adapter could double-act on the same fill event, causing incorrect position updates.

**v0.10 Status:** Partial handling. Basic deduplication existed but was incomplete and lacked TTL-based cleanup.

**v0.13 Implementation:**
- Added `OrderCallbackKey` struct for deduplication key (order_id, status, filled_quantity)
- Added `CallbackDedupCache` HashMap with TTL-based cleanup (5 minutes)
- Integrated deduplication check in `IbkrClient::execute_limit_order` before processing callbacks
- Added unit tests for duplicate detection and TTL cleanup

**Bug or Gap:** Gap in hardening. v0.10 had basic deduplication but it was incomplete and lacked proper cache management.

**Key Changes:**
- `broker/src/ibkr/client.rs`: Added callback_cache field with dedup methods
- `broker/src/ibkr/orders.rs`: Added OrderCallbackKey, CallbackDedupCache, and deduplication logic
- `rebalancer/src/broker.rs`: Updated execute_limit_order call with optional dedup_cache parameter

**Testing:** Unit tests in `broker/src/ibkr/orders.rs` (test_dedup_cache_detects_duplicates, test_dedup_cache_ttl_cleanup). Integration tests via MockTws failure injection.

### F2: Cancel Reject Race with Fill

**Problem:** Cancel requests can race against in-flight fills. When IBKR rejects a cancel (e.g., order already filled), the broker must reconcile that the order is filled and the cancel is moot.

**v0.10 Status:** None. Cancel rejections were not handled, leading to ambiguous state.

**v0.13 Implementation:**
- Added `BrokerError::CancelReject` variant with order_id and reason fields
- Changed `cancel_order` to return `Result<(), BrokerError>` instead of `()`
- Added `reconcile_filled_order` to infer order state from rejection reason
- Integrated reconciliation in `execute_limit_order` when cancel is rejected during timeout
- Added comprehensive audit logging with "AUDIT:" prefix for cancel attempts

**Bug or Gap:** New bug surfaced. This race condition was not previously handled.

**Key Changes:**
- `broker/src/error.rs`: Added CancelReject variant
- `broker/src/ibkr/orders.rs`: Added reconciliation logic and Result-based cancel_order
- `broker/src/ibkr/mod.rs`: Updated IbkrBroker::cancel_order to propagate Result type

**Testing:** 4 unit tests in `broker/src/ibkr/orders.rs` (fill detection, completed detection, uncertain reasons, case-insensitive matching). MockTws failure injection tests.

### F3: Partial Fill + Disconnect

**Problem:** An order partially fills, then TWS disconnects. On reconnect, the broker must query IBKR's open-positions for ground truth and not double-submit the remainder.

**v0.10 Status:** None. Disconnects during partial fills were not handled.

**v0.13 Implementation:**
- Added `BrokerError::ConnectionLost` variant with order_id and filled_quantity
- Added disconnect detection in `execute_limit_order` (errors 1100/1101/1102 and silent disconnects)
- Added `IbkrClient::reconnect` method to re-establish connection and query positions
- Added `reconcile_partial_fill` to query IBKR positions on reconnect for ground truth
- Deliberately does NOT resubmit remainder (manual review required)

**Bug or Gap:** New bug surfaced. Partial fills during disconnects were not handled.

**Key Changes:**
- `broker/src/error.rs`: Added ConnectionLost variant
- `broker/src/ibkr/client.rs`: Added reconnect method
- `broker/src/ibkr/orders.rs`: Added reconcile_partial_fill and disconnect detection
- `broker/src/ibkr/mod.rs`: Added reconnect method to IbkrBroker

**Testing:** 5 unit tests in `broker/src/ibkr/orders.rs` (additional fill detection, no additional fill, position not found, sell orders, ConnectionLost error variant). MockTws failure injection tests.

### F4: Stale Market Data Detection

**Problem:** A price snapshot is held while quotes move in the market. Trading on stale data can lead to poor fills or failed orders.

**v0.10 Status:** None. No staleness detection existed.

**v0.13 Implementation:**
- Added `timestamp` field to `Quote` struct (SystemTime)
- Added `Quote::is_stale` method to check if quote exceeds age threshold
- Added `ExecutionConfig::quote_staleness_threshold_sec` with default 30s
- Added `Error::StaleQuote` variant with symbol, age_sec, threshold_sec
- Integrated staleness check in execution.rs before order submission
- Changed from `client.prices()` to `client.quotes()` to get full Quote objects

**Bug or Gap:** New bug surfaced. Stale quotes were not detected.

**Key Changes:**
- `broker/src/types.rs`: Added timestamp field to Quote, is_stale method
- `rebalancer/src/config.rs`: Added quote_staleness_threshold_sec
- `rebalancer/src/error.rs`: Added StaleQuote variant
- `rebalancer/src/execution.rs`: Integrated staleness check before order submission
- `rebalancer/src/broker.rs`: Added quotes() method to BrokerGateway

**Testing:** 4 staleness detection tests in `rebalancer/tests/execution_integration.rs` (fresh, stale, boundary, clock skew). 4 Quote::is_stale unit tests.

### F5: Clock Skew Detection

**Problem:** NTP drift or VM clock jumps can corrupt audit-log timestamps or rebalance windowing, leading to incorrect state reconstruction or duplicate executions.

**v0.10 Status:** Partial. Basic clock handling existed but no skew detection.

**v0.13 Implementation:**
- Added `clock_skew.rs` module with `ClockSkewDetector` struct
- Detects backward jumps (clock went backward)
- Detects forward jumps (clock jumped forward too fast)
- Added `SkewResult` enum (Ok, BackwardJump, ForwardJump)
- Configurable thresholds (default: 30s backward, 2.0x forward rate)
- Integrated into AuditLog with WARN-level logging when skew detected

**Bug or Gap:** Enhancement to existing partial handling. v0.10 had basic clock handling but no skew detection.

**Key Changes:**
- `rebalancer/src/clock_skew.rs`: New module with ClockSkewDetector
- `rebalancer/src/audit.rs`: Integrated ClockSkewDetector
- `rebalancer/src/config.rs`: Added clock_skew_threshold_sec and max_jump_rate_sec_per_sec
- `rebalancer/src/lib.rs`: Added clock_skew module

**Testing:** 4 integration tests in `rebalancer/tests/` (detector init, backward jump, forward jump, logging continues despite skew). Unit tests for all detection scenarios.

### F6: TWS Restart Drill

**Problem:** TWS restarts while the rebalancer has open positions. The broker adapter must detect disconnect, reconnect, query IBKR state, reconcile against local state, and resume monitoring without double-submitting orders.

**v0.10 Status:** None. TWS restarts were not handled.

**v0.13 Implementation:**
- Added `ConnectionState` enum (Connected, Disconnected, Reconnecting)
- Added `reconnect_with_backoff` method with exponential backoff (1s, 2s, 4s, 8s, 16s max)
- Added `IbkrClient::open_orders()` using ibapi's `all_open_orders()` API
- Added `CachedOrder` struct and thread-safe order cache
- Added `IbkrBroker::reconcile_state()` method with discrepancy detection
- Added reconciliation safety checks (block submission when discrepancies detected)
- Extended MockTws with simulate_disconnect, simulate_reconnect, simulate_partial_fill

**Bug or Gap:** New bug surfaced. TWS restarts were not handled.

**Key Changes:**
- `broker/src/error.rs`: Added ReconnectFailed variant
- `broker/src/ibkr/client.rs`: Added open_orders, reconnect methods
- `broker/src/ibkr/mod.rs`: Added ConnectionState, reconcile_state
- `broker/src/ibkr/orders.rs`: Added order cache and reconciliation logic
- `broker/tests/`: Added ibkr_reconnect.rs, ibkr_state_query.rs, ibkr_reconcile.rs, ibkr_f6_reconnect_drill.rs

**Testing:** 23 new tests across 4 test suites (8 reconnect tests, 4 state query tests, 7 reconcile tests, 4 end-to-end drill tests). All verify no double-submit, 30s target, state persistence, orphan order detection.

### F7: Cron Double-Fire Idempotency

**Problem:** Cron misfires or manual re-run causes the rebalancer to execute the same window twice. Without idempotency, this could lead to duplicate orders.

**v0.10 Status:** None. No idempotency mechanism existed.

**v0.13 Implementation:**
- Added `--cron-mode` flag to Run command
- Added `CronMode` struct with sequence_number field
- Added sequence_number and window_id fields to AuditEvent
- Added `AuditLog::check_window_already_complete` to read audit log for completed windows
- Added `TargetSpec::window_id()` method to derive stable hash from target specification
- Integrated idempotency check at start of run() in cron mode
- Added `Error::IdempotencyRejection` variant

**Bug or Gap:** New bug surfaced. Cron double-fires were not prevented.

**Key Changes:**
- `rebalancer/src/main.rs`: Added --cron-mode flag
- `rebalancer/src/audit.rs`: Added sequence_number, window_id, check_window_already_complete
- `rebalancer/src/target.rs`: Added window_id() method
- `rebalancer/src/error.rs`: Added IdempotencyRejection variant
- `rebalancer/src/execution.rs`: Integrated idempotency check

**Testing:** 5 idempotency tests + 2 window_id stability tests. Integration tests verify duplicate runs are rejected.

### F8: Kill Switch

**Problem:** Emergency stop of a running runner is needed when something goes wrong. Without a kill switch, operators must manually find and kill the process, risking dangling orders.

**v0.10 Status:** None. No kill switch existed.

**v0.13 Implementation:**
- Added `--kill` subcommand to rebalancer
- Added `kill.rs` module with send_sigterm, verify_no_dangling_orders, run_kill
- Added `pid_file.rs` module for PID file management
- Uses nix crate for Unix signal handling
- Verifies no dangling orders remain on the exchange via audit log query
- Comprehensive documentation covering usage, error cases, manual intervention

**Bug or Gap:** New bug surfaced. No emergency stop mechanism existed.

**Key Changes:**
- `rebalancer/src/kill.rs`: New module with kill switch implementation
- `rebalancer/src/pid_file.rs`: New module for PID file management
- `rebalancer/src/main.rs`: Added Kill subcommand
- `rebalancer/Cargo.toml`: Added nix dependency (signal features)

**Testing:** 2 integration tests for order verification. 5 unit tests (3 kill + 2 pid_file).

### F9: Process Crash + Warm Restart

**Problem:** The rebalancer process crashes (SIGKILL) mid-rebalance. On restart, the system must read the audit log, reconstruct state at the failure point, and complete or roll back the rebalance cleanly without double-submitting orders.

**v0.10 Status:** None. No crash recovery mechanism existed.

**v0.13 Implementation:**
- Added audit log checkpoints (RunStarted, PositionsFetched, DiffComputed, RiskCheckPassed, OrderSubmitted, OrderFilled, RunCompleted)
- Added state reconstruction logic (reconstruct_state, RecoveredState struct)
- Added broker state query methods (query_open_orders, query_positions)
- Added `--recover` subcommand with --dry-run mode
- Added recovery action decision logic (Restart, Resume, ManualReview, Rollback)
- Integration tests for all 7 checkpoints

**Bug or Gap:** New bug surfaced. Process crashes were not recoverable.

**Key Changes:**
- `rebalancer/src/audit.rs`: Added Checkpoint enum, log_checkpoint
- `rebalancer/src/recovery.rs`: New module with reconstruct_state, recovery action logic
- `rebalancer/src/broker.rs`: Added open_orders, compare_broker_state
- `rebalancer/src/main.rs`: Added recover subcommand
- `rebalancer/tests/recovery_integration.rs`: 7 integration tests

**Testing:** 7 integration tests covering all checkpoints. All verify correct state reconstruction and recovery action determination. 92 total tests passing in nanobook-rebalancer.

## Cross-Cutting Learnings

### 1. Audit Log as Source of Truth

**Used in:** F7 (idempotency), F9 (crash recovery)

**Key Pattern:**
- Sequence numbers prevent double-fire (F7)
- Checkpoints enable state reconstruction (F9)
- Audit log is the single source of truth for what happened
- All significant events are logged with sequence numbers and timestamps

**Implementation Details:**
- AuditEvent includes sequence_number, window_id, checkpoint fields
- Sequence numbers are strictly monotonic
- Checkpoints mark progress through a rebalance run
- Audit log is append-only JSONL for easy parsing

**Benefits:**
- Idempotency: Detect duplicate executions by checking sequence numbers
- Recovery: Reconstruct state by reading audit log from beginning
- Debugging: Full history of what happened during a run
- Compliance: Complete audit trail for regulatory requirements

### 2. State Reconciliation Pattern

**Used in:** F3 (partial fill + disconnect), F6 (TWS restart), F9 (process crash)

**Key Pattern:**
1. Detect failure or disconnect
2. Reconnect to broker
3. Query broker state (open orders, positions)
4. Compare broker state against local state
5. Detect discrepancies (orphans, missing, mismatches)
6. Take action (resume, manual review, rollback)

**Implementation Details:**
- `IbkrBroker::reconcile_state()` compares broker vs local state
- `DiscrepancyReport` identifies orphan orders, missing orders, status mismatches, position mismatches
- Safety checks block order submission when reconciliation fails
- Manual review mode for ambiguous cases

**Benefits:**
- Prevents double-submits by detecting orphans
- Ensures local state matches broker state
- Enables safe recovery from crashes and disconnects
- Provides clear guidance for manual intervention

### 3. Graceful Degradation

**Used in:** All failure modes

**Key Pattern:**
- Broker connection failures don't crash the system
- Manual review mode for ambiguous cases
- Dry-run mode for preview
- Non-blocking warnings (e.g., clock skew detection continues logging)

**Implementation Details:**
- Errors return Result types instead of panicking
- Warnings logged at appropriate levels (WARN for clock skew, DEBUG for duplicate callbacks)
- --dry-run flag for preview without execution
- Manual review recovery action when state is ambiguous

**Benefits:**
- System continues operating despite failures
- Operators have time to investigate and intervene
- No silent failures or data corruption
- Clear error messages guide operator action

### 4. Testing Philosophy

**Used in:** All failure modes

**Key Pattern:**
- MockTws enables deterministic failure injection
- Integration tests verify end-to-end behavior
- Each failure mode has dedicated test coverage
- Unit tests for individual components

**Implementation Details:**
- MockTws harness (bd-23o) provides wire-level mock with 9 failure modes
- Failure injection methods: simulate_disconnect, simulate_reconnect, simulate_partial_fill
- Integration tests for each failure mode (F1-F9)
- Unit tests for reconciliation logic, deduplication, skew detection

**Benefits:**
- Deterministic testing of failure scenarios
- Fast feedback loop (no need for real TWS)
- Comprehensive coverage of edge cases
- Regression prevention for future changes

## Architectural Changes

### 1. Broker Trait Extensions

**Added Methods:**
- `open_orders()`: Query all open orders from broker
- `reconcile_state()`: Compare broker state against local state
- `quotes()`: Fetch full Quote objects with timestamps (not just prices)
- `reconnect()`: Re-establish connection after disconnect

**New Types:**
- `ConnectionState`: Track connection state (Connected, Disconnected, Reconnecting)
- `CachedOrder`: Thread-safe order cache for reconciliation
- `DiscrepancyReport`: Report differences between broker and local state
- `Quote`: Extended with timestamp field for staleness detection

**Benefits:**
- Generic state reconciliation across brokers
- Connection state tracking for all brokers
- Staleness detection for all brokers
- Consistent API for broker operations

### 2. Error Handling

**New Error Variants:**
- `BrokerError::CancelReject`: Cancel request rejected by broker
- `BrokerError::ConnectionLost`: Disconnect during order execution
- `BrokerError::ReconnectFailed`: Reconnect attempts exhausted
- `Error::StaleQuote`: Quote exceeds staleness threshold
- `Error::IdempotencyRejection`: Duplicate execution attempt rejected

**Error Context:**
- All errors include relevant fields (order_id, reason, symbol, age_sec, etc.)
- Comprehensive error messages for debugging
- Audit logging with "AUDIT:" prefix for operational events

**Benefits:**
- Better error context for debugging
- Structured error handling instead of panics
- Clear operator guidance for error resolution
- Audit trail of error events

### 3. CLI Extensions

**New Flags:**
- `--cron-mode`: Enable cron mode with idempotency checks (F7)
- `--kill`: Kill running runner via PID file (F8)
- `--recover`: Recover from crash using audit log (F9)
- `--dry-run`: Preview recovery action without execution (F9)

**New Subcommands:**
- `rebalancer kill`: Send SIGTERM to running runner
- `rebalancer recover`: Recover from crash using audit log

**Benefits:**
- Operator-friendly CLI for common operations
- Idempotency protection for cron jobs
- Emergency stop capability
- Crash recovery workflow

### 4. Audit Log Extensions

**New Fields:**
- `sequence_number`: Monotonic sequence number for ordering
- `window_id`: Stable hash of target specification for idempotency
- `checkpoint`: Checkpoint type for crash recovery

**New Events:**
- `run_started`: Rebalance run begins
- `positions_fetched`: Current positions retrieved from broker
- `diff_computed`: Rebalance diff computed
- `risk_check_passed`: Risk checks passed
- `order_submitted`: Individual order submitted
- `order_filled`: Individual order filled
- `run_completed`: Rebalance run completes

**New Methods:**
- `log_with_idempotency()`: Log with sequence number
- `check_window_already_complete()`: Check for duplicate executions
- `log_checkpoint()`: Log checkpoint events

**Benefits:**
- Complete audit trail for compliance
- Idempotency via sequence numbers
- State reconstruction via checkpoints
- Debugging via event history

## Recommendations

### 1. For v0.14

**Extend Failure Modes to Binance:**
- Implement F-bin1 (Binance idempotency proof) - mirror F7
- Implement F-bin2 (Binance reconnect drill) - mirror F6
- Use MockBinance for failure injection (similar to MockTws)

**Add Automated Recovery:**
- Cancel orphan orders automatically (currently requires manual review)
- Resume from checkpoint automatically when safe
- Add recovery action escalation (automatic -> manual -> operator)

**Add Monitoring/Alerting:**
- Alert on failure mode occurrences
- Track reconciliation frequency and patterns
- Monitor clock skew events
- Alert on audit log corruption

### 2. For Operators

**Regular Audit Log Review:**
- Review audit logs after each rebalance run
- Check for unexpected events or gaps in sequence numbers
- Verify that `run_completed` is present for successful runs
- Archive old audit logs for historical analysis

**Test Recovery Procedures:**
- Practice recovery procedures in a test environment
- Simulate different crash scenarios (process crash, TWS restart, network failure)
- Verify that `rebalancer recover` produces the expected output
- Ensure team members are familiar with the recovery process

**Monitor for Failure Mode Patterns:**
- Track frequency of each failure mode
- Identify patterns (e.g., specific times, symbols, market conditions)
- Escalate if failure modes occur frequently
- Update procedures based on lessons learned

### 3. For Developers

**Keep MockTws in Sync:**
- Update MockTws when TWS API changes
- Add new failure modes as they're discovered
- Ensure MockTws behavior matches real TWS
- Test against real TWS regularly

**Add New Failure Modes:**
- Document new failure modes as they're discovered
- Implement detection and recovery logic
- Add tests for new failure modes
- Update this document with learnings

**Maintain Test Coverage:**
- Each failure mode must have dedicated test coverage
- Unit tests for individual components
- Integration tests for end-to-end behavior
- Regression tests for bug fixes

## References

- [Warm Restart Guide](../ops/warm-restart.md) - Detailed guide for operators on crash recovery
- [Failure Modes Deepening Plan](../../plan_failure_modes_deepening.md) - Deepened implementation plan for F6, F9, F-bin1, F-bin2
- [Individual Failure Mode Beads](../../.beads/issues.jsonl) - bd-3jo, bd-3u6, bd-39t, bd-3sz, bd-3kp, bd-w7x, bd-fu8, bd-bkb, bd-1t4

## Appendix: Implementation Timeline

| Date | Failure Mode | Commit | Description |
|------|-------------|--------|-------------|
| 2026-05-12 | F1 | 196ec67 | Duplicate order-status callback deduplication |
| 2026-05-12 | F2 | 3e77464 | Cancel reject race with fill reconciliation |
| 2026-05-12 | F3 | f6290fd | Partial fill followed by disconnect reconciliation |
| 2026-05-13 | F4 | 5d75055 | Stale market data detection |
| 2026-05-13 | F5 | 55f94fa | Clock skew detection |
| 2026-05-13 | F6 | d94d146 | TWS restart drill (final phase) |
| 2026-05-13 | F7 | 9a863e2 | Cron double-fire idempotency |
| 2026-05-13 | F8 | ab1b0a2 | Kill switch |
| 2026-05-13 | F9 | f0dca02 | Process crash + warm restart (final phase) |

**Total Implementation Time:** ~2 days (May 12-13, 2026)

**Total Lines Added:** ~3,000 lines across broker and rebalancer crates

**Total Tests Added:** ~70 new tests (unit + integration)
