# Production Hardening Implementation Plan

## Overview

This plan implements production hardening for the nanobook rebalancer to address the most critical operational gaps: crash recovery, guaranteed kill switch, observability, and configuration validation.

**Strategic Goal:** Production hardening - make the rebalancer safe to run in live trading with proper recovery, safety mechanisms, and operational visibility.

**Implementation Order:**
- **Parallel:** Phase 1 (Crash Recovery) + Phase 2 (Guaranteed Kill Switch)
- **Sequential:** Phase 3 (Observability) after Phase 1+2 complete
- **Anytime:** Phase 4 (Configuration Validation) - low complexity, can run in parallel

---

## Phase 1: Crash Recovery (Write-Ahead Logging)

### Objective
Prevent duplicate orders and enable safe recovery from mid-rebalance crashes by implementing write-ahead logging for all state changes.

### Approach
- Log intent before external calls, log result after
- Append-only audit log updates (no in-place modifications)
- Recovery queries broker to reconcile intent vs actual state
- Retry with exponential backoff for transient failures
- Order safety first in recovery workflow

### Tasks

#### 1.1 Audit Log Extensions
- [ ] Add `OrderIntent` checkpoint enum variant to `rebalancer/src/audit.rs`
- [ ] Add `OrderFailed` checkpoint enum variant with failure reason field
- [ ] Update checkpoint sequence validation to include new checkpoints
- [ ] Add intent logging function `log_order_intent()` with full order metadata
- [ ] Add failure logging function `log_order_failed()` with error details

**Full intent metadata includes:**
- symbol, side, quantity, limit_price
- client_order_id
- timestamp
- target spec reference (window_id or target file hash)
- execution context (timeout)

#### 1.2 Write-Ahead Wrapper Function
- [ ] Create `execute_order_with_write_ahead()` in `rebalancer/src/execution.rs`
- [ ] Implement write-ahead flow: log intent → broker call → log success/failure
- [ ] Add retry logic with exponential backoff for broker call failures
- [ ] Handle `OrderIntent` → `OrderSubmitted` success path
- [ ] Handle `OrderIntent` → `OrderFailed` failure path with error details
- [ ] Replace direct `client.execute_limit_order()` calls with wrapper

**Signature:**
```rust
fn execute_order_with_write_ahead(
    broker: &mut impl Broker,
    audit: &mut AuditLog,
    order: &RebalanceOrder,
    client_order_id: &ClientOrderId,
    timeout: Duration,
) -> Result<OrderExecutionResult>
```

#### 1.3 Broker Reconciliation in Recovery
- [ ] Extend `recovery::compare_broker_state()` to handle `OrderIntent` events
- [ ] Implement broker query for orders with matching client_order_id
- [ ] Add retry with exponential backoff (1s, 2s, 4s, 8s, max 5 attempts)
- [ ] Update recovery state reconstruction to detect incomplete intents
- [ ] Add recovery action: query broker, update audit log with broker_id if found
- [ ] Add recovery action: mark as failed if not found at broker
- [ ] Update `determine_recovery_action()` to handle incomplete intents

**Recovery logic:**
- `OrderIntent` + `OrderSubmitted` = success
- `OrderIntent` + `OrderFailed` = known failure
- `OrderIntent` alone = ambiguous (query broker to resolve)

#### 1.4 Feature Flag
- [ ] Add `write_ahead_logging` feature flag to `rebalancer/Cargo.toml`
- [ ] Gate write-ahead wrapper function behind feature flag
- [ ] Gate recovery reconciliation logic behind feature flag
- [ ] Update tests to run with and without feature flag

#### 1.5 Testing
- [ ] Create integration test with crash injection
- [ ] Test crash points: after intent logging, after broker call, during audit write
- [ ] Verify recovery reconstructs correct state after each crash point
- [ ] Verify no duplicate orders after recovery
- [ ] Add golden fixture tests for audit log parsing
- [ ] Add unit tests for wrapper function with mock broker

**Crash injection approach:**
- Use signal handling or special "crash_checkpoint" config
- Test with real broker (or high-fidelity mock)
- Verify broker state matches audit log after recovery

#### 1.6 Phased Rollout
- [ ] **Phase 1A:** Implement write-ahead for order submission only
- [ ] Test order submission write-ahead in dry-run mode
- [ ] Enable for live execution with monitoring
- [ ] **Phase 1B:** Extend to positions and quotes
- [ ] Add intent logging for `broker.positions()` and `broker.quotes()`
- [ ] Test and validate
- [ ] **Phase 1C:** Extend to all state changes
- [ ] Add intent logging for account summary, order cancellation
- [ ] Final validation and monitoring

### Dependencies
- None (can start immediately)

### Estimated Effort
- Phase 1A (orders only): 3-5 days
- Phase 1B (positions/quotes): 2-3 days
- Phase 1C (all state changes): 2-3 days
- **Total:** 7-11 days

### Success Criteria
- [ ] Integration test with crash injection passes
- [ ] Recovery correctly handles all crash points
- [ ] No duplicate orders in any crash scenario
- [ ] Audit log sequence is valid after crashes
- [ ] Feature flag allows safe rollout

---

## Phase 2: Guaranteed Kill Switch

### Objective
Implement a guaranteed kill switch that cancels all orders and verifies cancellation, even if the process is stuck or unresponsive.

### Approach
- Two-phase kill: graceful (SIGTERM) → forceful (direct broker cancellation)
- Graceful: stop after current order, cancel remaining orders
- Forceful: query broker open orders, cancel all, verify with retry
- Audit trail: kill request + completion events

### Tasks

#### 2.1 Graceful Shutdown
- [ ] Implement SIGTERM handler in rebalancer process
- [ ] Add shutdown flag to detect kill request
- [ ] Implement "stop after current order" logic
  - If mid-order submission: wait for completion
  - If between orders: stop immediately
- [ ] Cancel remaining orders in queue after current order
- [ ] Write audit log entry for graceful shutdown
- [ ] Exit cleanly after cancellation

**Graceful shutdown flow:**
1. Receive SIGTERM
2. Set shutdown flag
3. If mid-order: wait for broker response
4. Cancel all pending orders
5. Write `KillCompleted` event to audit log
6. Exit

#### 2.2 Two-Phase Kill Workflow
- [ ] Add `KillPhase1Started` and `KillPhase1Completed` audit events
- [ ] Implement Phase 1: send SIGTERM, wait for graceful shutdown (30s timeout)
- [ ] Add `KillPhase2Started` audit event
- [ ] Implement Phase 2: connect to broker directly if Phase 1 times out
- [ ] Add `KillPhase2Completed` audit event

#### 2.3 Forceful Cancellation
- [ ] Implement broker query for open orders
- [ ] Cancel all open orders individually
- [ ] Implement verification with retry (1s, 2s, 4s, 8s, 16s, max 5 retries)
- [ ] Query open orders after each retry
- [ ] Log error with remaining order IDs if verification fails
- [ ] Write `KillForced` event to audit log if forceful cancellation used

**Forceful cancellation flow:**
1. Connect to broker
2. Query `open_orders()`
3. Cancel each order
4. Wait 1s, query `open_orders()` again
5. If not empty, retry cancellation for remaining orders
6. Repeat with exponential backoff (max 5 retries)
7. If still not empty, log error with remaining order IDs
8. Write `KillCompleted` or `KillForced` event

#### 2.4 Kill Switch Audit Events
- [ ] Add `KillRequested` event with metadata (trigger time, method)
- [ ] Add `KillCompleted` event with summary
  - method (graceful/forced)
  - orders_cancelled_count
  - orders_remaining_count
  - duration_seconds
  - error messages (if any)
- [ ] Update kill command to write audit events
- [ ] Update recovery to parse kill events

#### 2.5 Feature Flag
- [ ] Add `guaranteed_kill_switch` feature flag to `rebalancer/Cargo.toml`
- [ ] Gate two-phase kill logic behind feature flag
- [ ] Gate forceful cancellation logic behind feature flag
- [ ] Update tests to run with and without feature flag

#### 2.6 Testing
- [ ] Create test for graceful shutdown (SIGTERM handling)
- [ ] Create test for forceful cancellation (broker query + cancel)
- [ ] Create test for kill verification retry logic
- [ ] Integration test: kill during order submission
- [ ] Integration test: kill when process is stuck (mock stuck state)
- [ ] Verify audit log events are written correctly
- [ ] Test with both feature flags on (write-ahead + kill switch)

### Dependencies
- None (can start immediately, parallel with Phase 1)

### Estimated Effort
- Graceful shutdown: 2-3 days
- Two-phase workflow: 2-3 days
- Forceful cancellation: 2-3 days
- Testing: 2-3 days
- **Total:** 8-12 days

### Success Criteria
- [ ] Graceful shutdown stops after current order
- [ ] Forceful cancellation cancels all broker orders
- [ ] Verification retry logic handles transient failures
- [ ] Audit log captures kill sequence
- [ ] Integration tests pass for all kill scenarios
- [ ] Works with write-ahead logging enabled

---

## Phase 3: Observability (Structured Logging)

### Objective
Add structured logging with tracing to provide production visibility and enable debugging without SSH access.

### Approach
- Use tracing crate ecosystem (modern Rust standard)
- Hybrid span structure: run → phases → orders
- JSON-formatted logs with correlation IDs
- Path to future OpenTelemetry integration

### Tasks

#### 3.1 Tracing Infrastructure
- [ ] Add tracing dependencies to `rebalancer/Cargo.toml`
  - `tracing`
  - `tracing-subscriber`
  - `tracing-appender` (for file logging)
  - `tracing-log` (compatibility with existing log crate)
- [ ] Replace env_logger with tracing-subscriber in main.rs
- [ ] Configure JSON formatter for stdout
- [ ] Add file appender for audit log separation
- [ ] Add correlation ID generation for each rebalance run

#### 3.2 Span Structure
- [ ] Implement hybrid span structure
  - `rebalance_run` (top-level)
    - `connect_to_broker`
    - `fetch_positions`
    - `fetch_quotes`
    - `compute_diff`
    - `risk_check`
    - `execute_orders`
      - `submit_order` (per-order)
    - `reconcile`
- [ ] Add metadata to each span (relevant parameters, timing)
- [ ] Convert existing log statements to tracing macros
- [ ] Add span context to error messages

#### 3.3 Migration
- [ ] Convert `log::info!` → `tracing::info!`
- [ ] Convert `log::warn!` → `tracing::warn!`
- [ ] Convert `log::error!` → `tracing::error!`
- [ ] Add span guards (`#[span]` attribute or `span!` macro)
- [ ] Update log format to include span context
- [ ] Test that log output is valid JSON

#### 3.4 Testing
- [ ] Verify JSON log format is parseable
- [ ] Verify correlation IDs are consistent across spans
- [ ] Verify span hierarchy is correct
- [ ] Test log output in various scenarios (success, failure, crash)
- [ ] Performance test: ensure tracing doesn't add significant overhead

### Dependencies
- Must wait for Phase 1+2 to complete (structured logging helps debug both features)

### Estimated Effort
- Tracing infrastructure: 2-3 days
- Span structure implementation: 2-3 days
- Migration of existing logs: 2-3 days
- Testing: 1-2 days
- **Total:** 7-11 days

### Success Criteria
- [ ] All logs are valid JSON
- [ ] Correlation IDs link all log entries for a run
- [ ] Span hierarchy matches execution flow
- [ ] Log parsing works with standard JSON tools
- [ ] Performance overhead is minimal (<5% latency impact)

---

## Phase 4: Configuration Validation at Startup

### Objective
Validate configuration at startup to fail fast with actionable errors before any trading logic runs.

### Approach
- Strict validation: exit with error code on any failure
- Actionable error messages with line numbers
- Comprehensive checks: broker, risk limits, file permissions, disk space

### Tasks

#### 4.1 Validation Functions
- [ ] Create `validator.rs` module in `rebalancer/src/`
- [ ] Implement broker connectivity check
- [ ] Implement risk limits validation (value ranges, non-negative, reasonable bounds)
- [ ] Implement file permissions check (audit log directory writable)
- [ ] Implement disk space check (sufficient space for audit logs)
- [ ] Implement required fields validation (no missing critical config values)
- [ ] Implement network timeout validation (connection timeout reasonable)

#### 4.2 Startup Validation
- [ ] Add validation call at start of `main()` before config loading
- [ ] Implement actionable error messages with file/line references
- [ ] Exit with non-zero status code on validation failure
- [ ] Add `--skip-validation` flag for testing (not for production)

#### 4.3 Error Messages
- [ ] Ensure each validation failure has clear, actionable error
- [ ] Include config file path and line number in errors
- [ ] Include suggested fixes in error messages where applicable
- [ ] Test error messages for clarity

**Example error message:**
```
Config validation failed:
  - risk.max_position_pct (150%) exceeds maximum (100%)
    Location: config.toml:42
    Fix: Set risk.max_position_pct to a value between 0 and 100
```

#### 4.4 Testing
- [ ] Test each validation function with valid and invalid inputs
- [ ] Test startup validation with various config errors
- [ ] Verify error messages are actionable
- [ ] Test `--skip-validation` flag works
- [ ] Integration test: validation fails, process exits

### Dependencies
- None (low complexity, can run in parallel with any phase)

### Estimated Effort
- Validation functions: 2-3 days
- Startup integration: 1-2 days
- Error message refinement: 1 day
- Testing: 1-2 days
- **Total:** 5-8 days

### Success Criteria
- [ ] All validation checks pass with valid config
- [ ] Validation fails with clear error messages for invalid config
- [ ] Process exits with non-zero code on validation failure
- [ ] Error messages include file/line references
- [ ] No config errors discovered mid-execution

---

## Implementation Strategy

### Parallel Development
- **Phase 1** (Crash Recovery) and **Phase 2** (Kill Switch) developed in parallel
- Independent teams or developers can work on each phase
- Feature flags allow independent testing and rollout
- Integration tests verify compatibility when both are complete

### Feature Flags
- `write_ahead_logging` - Phase 1
- `guaranteed_kill_switch` - Phase 2
- Both features default to off during development
- Rollout strategy: enable in dry-run mode first, then live execution
- Test matrix: both off, recovery on/kill off, recovery off/kill on, both on

### Testing Strategy
- Independent test suites for each phase
- Integration tests for feature flag combinations
- Contract tests for audit log events shared between features
- Continuous integration runs all test combinations

### Rollout Plan
1. **Development:** Implement with feature flags off
2. **Testing:** Enable feature flags in test environment
3. **Dry-run:** Enable in production dry-run mode (no real orders)
4. **Live:** Enable for live execution with monitoring
5. **Monitor:** Watch for issues, be ready to disable flags

### Risk Mitigation
- Feature flags allow instant rollback if issues occur
- Comprehensive testing before production enablement
- Phased rollout (dry-run → live) reduces blast radius
- Audit log provides trail for debugging issues

---

## Timeline

**Parallel Track (Weeks 1-2):**
- Phase 1A: Write-ahead for orders (3-5 days)
- Phase 2: Guaranteed kill switch (8-12 days)
- Phase 4: Config validation (5-8 days) - can start anytime

**Sequential Track (Weeks 3-4):**
- Phase 1B: Write-ahead for positions/quotes (2-3 days)
- Phase 1C: Write-ahead for all state changes (2-3 days)
- Phase 3: Structured logging (7-11 days)

**Total Estimated Time:** 4-5 weeks

---

## Success Metrics

### Phase 1 (Crash Recovery)
- Crash recovery test passes for all crash points
- Zero duplicate orders in crash scenarios
- Recovery time < 30 seconds for typical crashes
- Audit log remains valid after crashes

### Phase 2 (Kill Switch)
- Kill switch cancels all orders in < 60 seconds
- Forceful cancellation succeeds even when process is stuck
- Audit log captures complete kill sequence
- No orphan orders after kill

### Phase 3 (Observability)
- All logs parseable as JSON
- Correlation IDs link all entries for a run
- Can debug issues without SSH access
- Log overhead < 5% latency impact

### Phase 4 (Config Validation)
- Zero config errors discovered mid-execution
- Validation time < 5 seconds
- All error messages actionable
- No false positives in validation

---

## Open Questions

1. **Crash injection testing:** Should we use a real broker or a high-fidelity mock for crash injection tests?
2. **Feature flag defaults:** Should feature flags default to on or off in production builds?
3. **Monitoring:** What metrics should we add for monitoring these features in production?
4. **Rollback criteria:** What specific conditions would trigger rollback of feature flags?
5. **Documentation:** How much operational documentation is needed for these features?

---

## Next Steps

1. Review this plan with stakeholders
2. Assign developers to Phase 1 and Phase 2 (parallel tracks)
3. Set up feature flag infrastructure
4. Begin implementation with Phase 1A (orders write-ahead)
5. Schedule weekly syncs to track parallel progress
6. Plan integration testing when Phase 1+2 are complete
