# Deepening Plan: Complicated Failure Mode Features

## Overview

This plan deepens the implementation details for four complicated failure mode features that were originally stubbed out in v0.13. These features require substantial broker adapter work, audit log recovery logic, and state reconciliation mechanisms that were not fully specified in the initial bead descriptions.

## Target Beads

1. **bd-w7x**: v0.13 F6 - TWS restart mid-position (reconnect drill)
2. **bd-1t4**: v0.13 F9 - process crash mid-rebalance + warm restart
3. **bd-2ln**: v0.13 F-bin1 - Binance idempotency proof
4. **bd-1pd**: v0.13 F-bin2 - Binance reconnect drill

## Current State Assessment

### What Exists Today

- **MockTws harness**: Full wire-level mock with 9 failure modes (F1-F9)
- **Integration test stubs**: Tests exist but only verify error injection, not actual recovery
- **Binance broker adapter**: Partial implementation with basic REST operations
- **Audit log**: JSONL append-only log with sequence numbers for cron idempotency
- **No actual recovery logic**: Reconnect, warm restart, and state reconciliation are not implemented

### What's Missing

- **IBKR reconnect logic**: No code to query open-positions endpoint and reconcile state
- **Audit log recovery**: No code to read audit log, determine failure point, and resume
- **Binance WebSocket**: No WebSocket implementation for real-time updates
- **Binance failure modes**: No failure injection harness for Binance
- **State reconciliation**: No generic state reconciliation framework

---

## bd-w7x: v0.13 F6 - TWS Restart Mid-Position (Reconnect Drill)

### Problem Statement

When TWS restarts while the rebalancer has open positions, the broker adapter must:
1. Detect the disconnect
2. Reconnect to TWS
3. Query IBKR's open-positions endpoint for ground truth
4. Reconcile local state against broker state
5. Resume monitoring without double-submitting orders

**Target**: Reconcile within 30s of TWS coming back up.

### Deepened Implementation Plan

#### Phase 1: Disconnect Detection (2-3 days)

**Objective**: Add robust disconnect detection to IBKR broker adapter.

**Tasks**:
1. Add heartbeat mechanism to IBKR client
   - Send periodic ping/pong messages via EWrapper
   - Detect heartbeat timeout (e.g., 10s without response)
   - Mark connection as disconnected on timeout

2. Add connection state tracking
   - Track connection state: `Connected`, `Disconnected`, `Reconnecting`
   - Add `is_connected()` method to IBKRBroker
   - Add connection event callbacks for external monitoring

3. Implement auto-reconnect logic
   - Exponential backoff: 1s, 2s, 4s, 8s, 16s (max)
   - Max reconnect attempts: 5
   - Return error if reconnect fails after max attempts

**Verification**:
- Integration test: Simulate TWS disconnect, verify detection within 15s
- Unit test: Heartbeat timeout triggers disconnect state
- Manual test: Kill TWS process, observe reconnect attempts in logs

#### Phase 2: Open Positions Query (2-3 days)

**Objective**: Implement IBKR open-positions endpoint query.

**Tasks**:
1. Add `reqAllOpenOrders()` and `openOrder()` EWrapper methods
   - Request all open orders from TWS
   - Parse order status callbacks
   - Build local order cache

2. Add `reqPositions()` and `position()` EWrapper methods
   - Request all positions from TWS
   - Parse position callbacks
   - Build local position cache

3. Add `reconcile_state()` method to IBKRBroker
   - Query open orders from TWS
   - Query positions from TWS
   - Compare against local state (if any)
   - Return reconciliation report

**Verification**:
- Integration test: Submit order, query open orders, verify match
- Integration test: Open position, query positions, verify match
- Manual test: Run against paper account, verify reconciliation report

#### Phase 3: State Reconciliation (3-4 days)

**Objective**: Implement state reconciliation logic to prevent double-submits.

**Tasks**:
1. Define reconciliation invariants
   - Order ID uniqueness: No duplicate order IDs
   - Position consistency: Local positions match broker positions
   - Fill consistency: Filled quantities match broker fills

2. Implement reconciliation algorithm
   ```
   For each local order:
     - Check if order exists in broker open orders
     - If missing: check if it was filled/cancelled
     - If in unknown state: mark for manual review

   For each broker open order:
     - Check if order exists in local state
     - If missing: add to local state (orphan detection)
     - If quantity differs: mark for manual review
   ```

3. Add reconciliation safety checks
   - Block order submission if reconciliation fails
   - Require manual acknowledgment for orphan orders
   - Log all reconciliation discrepancies

**Verification**:
- Integration test: Simulate orphan order, verify detection
- Integration test: Simulate partial fill, verify reconciliation
- Manual test: Kill TWS mid-order, verify safe reconnect

#### Phase 4: Reconnect Drill Test (2 days)

**Objective**: End-to-end test of F6 failure mode.

**Tasks**:
1. Extend MockTws to support reconnect drill
   - Add `simulate_disconnect()` method
   - Add `simulate_reconnect()` method
   - Add state persistence across disconnect/reconnect

2. Implement F6 integration test
   - Submit order, fill partially
   - Inject disconnect
   - Trigger reconnect
   - Verify state reconciliation
   - Verify no double-submit on resume

3. Add 30s target measurement
   - Measure time from reconnect trigger to reconciliation complete
   - Assert reconciliation completes within 30s
   - Log reconciliation timing metrics

**Verification**:
- Integration test passes consistently
- Manual test against paper account
- Performance test: Reconciliation completes within 30s target

### Dependencies

- MockTws harness (bd-23o) - ✅ Complete
- IBKR broker adapter (existing)
- Audit log (existing)

### Risks & Mitigations

**Risk**: TWS API rate limits on open orders query
- **Mitigation**: Cache open orders, query only on reconnect
- **Mitigation**: Use batch queries where possible

**Risk**: Reconciliation logic is complex and error-prone
- **Mitigation**: Extensive unit and integration tests
- **Mitigation**: Manual review mode for ambiguous cases

**Risk**: 30s target may not be achievable with many open orders
- **Mitigation**: Measure actual performance, adjust target if needed
- **Mitigation**: Parallelize queries where possible

---

## bd-1t4: v0.13 F9 - Process Crash Mid-Rebalance + Warm Restart

### Problem Statement

When the rebalancer process crashes (SIGKILL) mid-rebalance, the restart must:
1. Read the audit log to determine the exact point of failure
2. Reconstruct state at failure point
3. Complete or roll back the rebalance cleanly
4. Prevent double-submission of orders

**Test**: Kill at every audit-log checkpoint, assert correct final state.

### Deepened Implementation Plan

#### Phase 1: Audit Log Checkpoints (2-3 days)

**Objective**: Define and implement audit-log checkpoints for recovery.

**Tasks**:
1. Define checkpoint events
   - `run_started`: Rebalance run begins
   - `positions_fetched`: Current positions retrieved
   - `diff_computed`: Rebalance diff computed
   - `risk_check_passed`: Risk checks passed
   - `order_submitted_<symbol>`: Individual order submitted
   - `order_filled_<symbol>`: Individual order filled
   - `run_completed`: Rebalance run completes (success or failure)

2. Add checkpoint logging to rebalancer
   - Log checkpoint events with sequence numbers
   - Include sufficient state in each checkpoint for recovery
   - Ensure atomic writes (fsync after each checkpoint)

3. Add checkpoint validation
   - Verify checkpoint sequence is monotonic
   - Verify no missing checkpoints between known states
   - Detect corrupted checkpoint entries

**Verification**:
- Unit test: Checkpoint sequence validation
- Integration test: Simulate crash at each checkpoint
- Manual test: Review audit log after crash

#### Phase 2: State Reconstruction (3-4 days)

**Objective**: Implement audit-log-driven state reconstruction.

**Tasks**:
1. Define recoverable state structure
   ```rust
   struct RecoveredState {
       checkpoint: Checkpoint,
       positions: Vec<Position>,
       orders: Vec<Order>,
       equity: i64,
       sequence_number: u64,
   }
   ```

2. Implement state reconstruction algorithm
   ```
   Read audit log from beginning to end:
     - Parse each checkpoint event
     - Update in-memory state
     - Track last known good checkpoint
     - Detect incomplete operations (e.g., order submitted but not filled)

   Determine recovery action:
     - If crash before order submission: safe to restart from beginning
     - If crash after order submission but before fill: query broker state
     - If crash after fill: mark order as complete, continue with remaining
   ```

3. Add recovery action decision logic
   - `Restart`: Safe to restart entire rebalance
   - `Resume`: Resume from last checkpoint
   - `ManualReview`: Requires operator intervention
   - `Rollback`: Rollback submitted orders (if possible)

**Verification**:
- Unit test: State reconstruction for each checkpoint
- Integration test: Simulate crash, verify correct recovery action
- Manual test: Crash at each checkpoint, verify recovery

#### Phase 3: Broker State Query (2-3 days)

**Objective**: Query broker state to resolve ambiguous recovery cases.

**Tasks**:
1. Add broker state query methods
   - `query_open_orders()`: Get all open orders from broker
   - `query_positions()`: Get all positions from broker
   - `query_order_status(order_id)`: Get status of specific order

2. Implement broker state comparison
   - Compare reconstructed state against broker state
   - Detect discrepancies (orphan orders, missing fills)
   - Generate discrepancy report

3. Add broker state reconciliation
   - For orphan orders: cancel or acknowledge
   - For missing fills: update local state
   - For discrepancies: flag for manual review

**Verification**:
- Integration test: Query broker state after crash
- Integration test: Reconcile broker state with reconstructed state
- Manual test: Crash during order submission, verify broker query

#### Phase 4: Warm Restart Implementation (3-4 days)

**Objective**: Implement warm restart logic in rebalancer binary.

**Tasks**:
1. Add `--recover` flag to rebalancer CLI
   - Read audit log on startup
   - Reconstruct state
   - Determine recovery action
   - Execute recovery or prompt for manual review

2. Implement recovery execution
   - For `Restart`: Start new rebalance from beginning
   - For `Resume`: Resume from last checkpoint
   - For `ManualReview`: Print recovery report, exit
   - For `Rollback`: Cancel orphan orders, restart

3. Add recovery logging
   - Log recovery action taken
   - Log state reconstruction details
   - Log broker state comparison results

**Verification**:
- Integration test: `--recover` flag with each crash scenario
- Manual test: Kill process, run with `--recover`, verify correct action
- End-to-end test: Full crash recovery cycle

#### Phase 5: F9 Integration Test (2 days)

**Objective**: End-to-end test of F9 failure mode.

**Tasks**:
1. Extend MockTws to support process crash simulation
   - Add `simulate_crash()` method
   - Add state persistence across crash
   - Add crash injection at specific checkpoints

2. Implement F9 integration test
   - Start rebalancer with mock broker
   - Inject crash at each checkpoint
   - Restart rebalancer with `--recover`
   - Verify correct final state
   - Verify no double-submission

3. Add checkpoint coverage test
   - Test crash at every checkpoint
   - Assert correct recovery for each
   - Measure recovery time for each checkpoint

**Verification**:
- Integration test passes for all checkpoints
- Manual test: Kill actual process, verify recovery
- Recovery time acceptable for production use

### Dependencies

- Audit log (existing)
- IBKR broker adapter (existing)
- F6 reconnect logic (bd-w7x) - blocks state query

### Risks & Mitigations

**Risk**: Audit log corruption prevents recovery
- **Mitigation**: Validate audit log on startup
- **Mitigation**: Keep backup of previous audit log

**Risk**: State reconstruction logic is complex
- **Mitigation**: Extensive unit tests for each checkpoint
- **Mitigation**: Manual review mode for ambiguous cases

**Risk**: Broker state query may fail during recovery
- **Mitigation**: Retry logic with exponential backoff
- **Mitigation**: Manual review mode if query fails

**Risk**: Recovery action may be incorrect
- **Mitigation**: Conservative defaults (e.g., manual review)
- **Mitigation**: Operator approval required for destructive actions

---

## bd-2ln: v0.13 F-bin1 - Binance Idempotency Proof

### Problem Statement

Mirror v0.13 F7 (cron double-fire idempotency) for Binance broker adapter. The rebalancer may be triggered twice (e.g., cron misconfiguration, manual re-run). The adapter must detect double-fire via sequence-number/audit-log check and ensure exactly one set of orders is submitted to Binance.

**Scope**: Lighter than IBKR full 9 modes — Codex review consensus was "at least 2" for Binance.

### Deepened Implementation Plan

#### Phase 1: Binance Order Cache (2 days)

**Objective**: Add local order cache to Binance broker adapter.

**Tasks**:
1. Define order cache structure
   ```rust
   struct BinanceOrderCache {
       orders: HashMap<OrderId, CachedOrder>,
   }

   struct CachedOrder {
       symbol: Symbol,
       quantity: i64,
       side: BrokerSide,
       status: OrderState,
       binance_order_id: String,
       client_order_id: Option<String>,
       submitted_at: DateTime<Utc>,
   }
   ```

2. Implement order cache persistence
   - Serialize cache to JSON on disk
   - Load cache on broker initialization
   - Update cache on order submission/fill/cancel

3. Add cache management methods
   - `cache_order()`: Cache order details on submission
   - `update_cached_order()`: Update order status on fill/cancel
   - `clear_cache()`: Clear cache on explicit request

**Verification**:
- Unit test: Order cache serialization/deserialization
- Integration test: Cache persistence across broker restart
- Manual test: Submit order, verify cache updated

#### Phase 2: Binance Client Order IDs (2 days)

**Objective**: Implement client order IDs for idempotency.

**Tasks**:
1. Add client order ID generation
   - Generate UUID-based client order IDs
   - Include sequence number in client order ID
   - Ensure uniqueness across runs

2. Modify order submission to use client order IDs
   - Pass `clientOrderId` to Binance API
   - Store client order ID in cache
   - Log client order ID in audit log

3. Add client order ID query
   - Query order by client order ID
   - Detect duplicate submission attempts
   - Return error on duplicate detection

**Verification**:
- Integration test: Submit order with client order ID
- Integration test: Duplicate submission detection
- Manual test: Re-run rebalancer, verify no duplicate orders

#### Phase 3: Audit Log Integration (2 days)

**Objective**: Integrate Binance adapter with audit log for idempotency.

**Tasks**:
1. Add audit log to Binance broker
   - Pass audit log handle to broker
   - Log order submissions with sequence numbers
   - Log order fills/cancels

2. Implement sequence number tracking
   - Generate sequence number on each run
   - Include sequence number in audit log events
   - Check for duplicate sequence numbers

3. Implement double-fire detection
   ```
   On order submission:
     - Check audit log for existing submission with same sequence number
     - If found: return error, do not submit order
     - If not found: submit order, log to audit log
   ```

**Verification**:
- Integration test: Double-fire detection via audit log
- Integration test: Sequence number tracking
- Manual test: Re-run rebalancer, verify idempotency

#### Phase 4: Binance Mock for Failure Injection (2-3 days)

**Objective**: Create Binance mock for testing failure modes.

**Tasks**:
1. Define Binance mock structure
   ```rust
   struct MockBinance {
       orders: HashMap<String, MockOrder>,
       client_order_ids: HashSet<String>,
   }

   struct MockOrder {
       symbol: String,
       quantity: String,
       side: String,
       status: OrderState,
       client_order_id: Option<String>,
   }
   ```

2. Implement mock Binance API
   - Mock REST endpoints: `/api/v3/order`, `/api/v3/openOrders`, etc.
   - Support client order ID deduplication
   - Simulate failure modes (disconnect, timeout, etc.)

3. Add mock to test harness
   - Integrate mock with broker tests
   - Add failure injection methods
   - Add state inspection methods

**Verification**:
- Unit test: Mock Binance API responses
- Integration test: Binance broker with mock
- Manual test: Verify mock behavior matches real API

#### Phase 5: F-bin1 Integration Test (2 days)

**Objective**: End-to-end test of Binance idempotency.

**Tasks**:
1. Implement F-bin1 integration test
   - Submit order with sequence number N
   - Attempt to submit same order again with sequence number N
   - Verify second submission is rejected
   - Verify only one order exists in mock

2. Add sequence number collision test
   - Submit order with sequence number N
   - Submit different order with sequence number N
   - Verify second submission is rejected

3. Add audit log verification
   - Verify audit log contains only one submission per sequence number
   - Verify audit log contains idempotency rejection event

**Verification**:
- Integration test passes
- Manual test: Re-run rebalancer with mock, verify idempotency

### Dependencies

- Binance broker adapter (partial implementation exists)
- Audit log (existing)
- MockTws harness (for reference)

### Risks & Mitigations

**Risk**: Binance API may not support client order IDs as expected
- **Mitigation**: Test against Binance testnet early
- **Mitigation**: Fallback to order ID-based deduplication

**Risk**: Order cache may become inconsistent with broker state
- **Mitigation**: Reconcile cache with broker state on startup
- **Mitigation**: Clear cache on explicit request

**Risk**: Audit log may not be available in all contexts
- **Mitigation**: Make audit log optional for testing
- **Mitigation**: In-memory fallback for unit tests

---

## bd-1pd: v0.13 F-bin2 - Binance Reconnect Drill

### Problem Statement

Mirror v0.13 F6 (TWS restart) for Binance: WebSocket drop + reconnect during open positions. The adapter must reconcile state from Binance account-info endpoint and prevent double-submits. Same 30s reconcile target as IBKR.

### Deepened Implementation Plan

#### Phase 1: Binance WebSocket Implementation (3-4 days)

**Objective**: Implement Binance WebSocket for real-time updates.

**Tasks**:
1. Add WebSocket client dependency
   - Add `tungstenite` or `tokio-tungstenite` crate
   - Add async runtime support (tokio)
   - Design WebSocket message handling

2. Implement Binance WebSocket connection
   - Connect to Binance WebSocket endpoint
   - Handle authentication (if required)
   - Subscribe to user data stream (account updates)

3. Implement WebSocket message handling
   - Parse account update messages
   - Parse execution report messages
   - Update local state on WebSocket events

**Verification**:
- Integration test: WebSocket connection to Binance testnet
- Integration test: Message parsing and handling
- Manual test: Connect to Binance testnet, verify updates

#### Phase 2: Disconnect Detection (2 days)

**Objective**: Add disconnect detection to Binance WebSocket.

**Tasks**:
1. Add heartbeat mechanism
   - Send periodic ping messages
   - Detect heartbeat timeout
   - Mark connection as disconnected on timeout

2. Add connection state tracking
   - Track connection state: `Connected`, `Disconnected`, `Reconnecting`
   - Add `is_connected()` method to BinanceBroker
   - Add connection event callbacks

3. Implement auto-reconnect logic
   - Exponential backoff: 1s, 2s, 4s, 8s, 16s (max)
   - Max reconnect attempts: 5
   - Re-subscribe to data streams on reconnect

**Verification**:
- Integration test: Simulate disconnect, verify detection
- Integration test: Auto-reconnect logic
- Manual test: Kill network connection, observe reconnect

#### Phase 3: Account Info Query (2-3 days)

**Objective**: Implement Binance account-info endpoint query for reconciliation.

**Tasks**:
1. Add account info query method
   - Call `/api/v3/account` endpoint
   - Parse balances and positions
   - Parse open orders

2. Implement state reconciliation
   - Query account info on reconnect
   - Compare against local state
   - Detect discrepancies (orphan orders, missing fills)

3. Add reconciliation safety checks
   - Block order submission if reconciliation fails
   - Require manual acknowledgment for orphan orders
   - Log all reconciliation discrepancies

**Verification**:
- Integration test: Query account info, verify parsing
- Integration test: Reconcile state after reconnect
- Manual test: Disconnect/reconnect against testnet

#### Phase 4: WebSocket + REST Fallback (2 days)

**Objective**: Implement REST API fallback when WebSocket is unavailable.

**Tasks**:
1. Add REST polling mode
   - Poll `/api/v3/account` endpoint periodically
   - Poll `/api/v3/openOrders` endpoint periodically
   - Update local state from REST responses

2. Implement automatic fallback
   - Switch to REST mode on WebSocket failure
   - Switch back to WebSocket when available
   - Log mode transitions

3. Add mode configuration
   - Allow manual mode selection (WebSocket/REST/Auto)
   - Default to Auto mode
   - Document tradeoffs of each mode

**Verification**:
- Integration test: REST polling mode
- Integration test: Auto fallback logic
- Manual test: Force WebSocket failure, verify REST fallback

#### Phase 5: F-bin2 Integration Test (2 days)

**Objective**: End-to-end test of Binance reconnect drill.

**Tasks**:
1. Extend mock Binance to support reconnect drill
   - Add `simulate_disconnect()` method
   - Add `simulate_reconnect()` method
   - Add state persistence across disconnect/reconnect

2. Implement F-bin2 integration test
   - Submit order via WebSocket
   - Inject disconnect
   - Trigger reconnect
   - Verify state reconciliation via account-info query
   - Verify no double-submit on resume

3. Add 30s target measurement
   - Measure time from reconnect trigger to reconciliation complete
   - Assert reconciliation completes within 30s
   - Log reconciliation timing metrics

**Verification**:
- Integration test passes consistently
- Manual test against Binance testnet
- Performance test: Reconciliation completes within 30s target

### Dependencies

- Binance broker adapter (partial implementation exists)
- F-bin1 idempotency (bd-2ln) - blocks state tracking
- Audit log (existing)

### Risks & Mitigations

**Risk**: Binance WebSocket API may be unstable
- **Mitigation**: Implement robust fallback to REST
- **Mitigation**: Extensive testing against testnet

**Risk**: Account info query rate limits
- **Mitigation**: Cache account info, query only on reconnect
- **Mitigation**: Use WebSocket for real-time updates

**Risk**: 30s target may not be achievable with Binance API
- **Mitigation**: Measure actual performance, adjust target if needed
- **Mitigation**: Optimize query batching

---

## Cross-Cutting Concerns

### Testing Strategy

**Unit Tests**:
- Mock broker responses
- Test individual components in isolation
- Fast feedback loop

**Integration Tests**:
- Test broker adapter with mock
- Test failure injection scenarios
- Test recovery logic

**Manual Tests**:
- Test against paper/testnet accounts
- Verify real-world behavior
- Catch integration issues

**Performance Tests**:
- Measure reconciliation timing
- Verify 30s target is achievable
- Identify bottlenecks

### Documentation Requirements

**Operator Documentation**:
- How to handle reconnect failures
- How to review reconciliation reports
- How to perform manual recovery
- Troubleshooting guide

**Developer Documentation**:
- Failure mode implementation details
- Audit log checkpoint specification
- State reconstruction algorithm
- Testing procedures

### Security Considerations

**PII Protection**:
- Use `scripts/sanitize-audit.py` before sharing audit logs
- Scrub account IDs, order IDs from logs
- Validate audit log scrubbing in CI

**Credential Safety**:
- Use environment variables for API keys
- Zeroize credentials on drop (already implemented)
- Never log credentials

**Audit Log Integrity**:
- Validate audit log on startup
- Detect tampering or corruption
- Keep backups of critical logs

### Error Handling Strategy

**Graceful Degradation**:
- Fallback to REST when WebSocket fails
- Manual review mode when reconciliation is ambiguous
- Continue operation with warnings when possible

**Conservative Defaults**:
- Prefer manual review over automatic recovery
- Block operations when state is ambiguous
- Log all discrepancies for operator review

**Retry Logic**:
- Exponential backoff for transient failures
- Max retry limits to prevent infinite loops
- Clear error messages for operator action

---

## Timeline Estimate

**Total Effort**: ~40-50 days

**Parallelization Opportunities**:
- bd-w7x (F6) and bd-1t4 (F9) can proceed in parallel after audit log checkpoints
- bd-2ln (F-bin1) and bd-1pd (F-bin2) can proceed in parallel after Binance broker basics
- Testing phases can overlap with implementation

**Critical Path**:
1. Audit log checkpoints (bd-1t4 Phase 1) - 3 days
2. State reconstruction (bd-1t4 Phase 2) - 4 days
3. IBKR reconnect logic (bd-w7x) - 8 days
4. Binance broker basics (bd-2ln Phases 1-2) - 4 days
5. Binance WebSocket (bd-1pd Phase 1) - 4 days

**Recommended Sequence**:
1. Start with bd-1t4 (F9) - foundational audit log recovery
2. Parallel: bd-w7x (F6) - IBKR reconnect logic
3. Parallel: bd-2ln (F-bin1) - Binance idempotency
4. After bd-2ln: bd-1pd (F-bin2) - Binance reconnect drill

---

## Success Criteria

Each bead is complete when:

1. **Implementation Complete**: All phases implemented and tested
2. **Integration Tests Pass**: All integration tests pass consistently
3. **Manual Tests Pass**: Manual tests against paper/testnet accounts pass
4. **Documentation Complete**: Operator and developer documentation written
5. **Performance Targets Met**: Reconciliation completes within 30s target
6. **Security Review Passed**: PII protection and credential safety verified
7. **Code Review Passed**: Code reviewed and approved
8. **Bead Updated**: Bead description updated with implementation notes

---

## Open Questions

1. **Binance Testnet Access**: Do we have Binance testnet access for testing?
2. **WebSocket vs REST**: Should we prioritize WebSocket or REST for Binance?
3. **Reconciliation Target**: Is 30s reconciliation target achievable for Binance?
4. **Manual Review Workflow**: What is the workflow for manual review of reconciliation?
5. **Audit Log Retention**: How long should we retain audit logs for recovery?

---

## Next Steps

1. **Review this plan** with stakeholders to confirm approach
2. **Answer open questions** through research or discussion
3. **Update beads** with deepened descriptions from this plan
4. **Begin implementation** with bd-1t4 (F9) as priority
5. **Set up testing infrastructure** for manual tests against paper/testnet
