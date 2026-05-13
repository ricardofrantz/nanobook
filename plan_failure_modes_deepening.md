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

#### Phase 1: Disconnect Detection

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
- `cargo test -p nanobook-broker --test ibkr_reconnect` passes
- Assertion: `IBKRBroker::is_connected()` returns `false` within 15s of `MockTws::simulate_disconnect()` — see `broker/tests/ibkr_reconnect.rs::test_heartbeat_timeout`
- Assertion: after max reconnect attempts exhausted, `IBKRBroker::connect()` returns `Err(BrokerError::ReconnectFailed)` — see `broker/tests/ibkr_reconnect.rs::test_reconnect_failure`

#### Phase 2: Open Positions Query

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
- `cargo test -p nanobook-broker --test ibkr_state_query` passes
- Assertion: `IBKRBroker::open_orders()` returns the same set submitted via `MockTws` — see `broker/tests/ibkr_state_query.rs::test_open_orders_roundtrip`
- Assertion: `IBKRBroker::positions()` reflects the partial fill injected by `MockTws::simulate_partial_fill()` — see `broker/tests/ibkr_state_query.rs::test_positions_after_partial_fill`

#### Phase 3: State Reconciliation

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
- `cargo test -p nanobook-broker --test ibkr_reconcile` passes
- Assertion: orphan order (present in broker, absent in local state) is detected and flagged — see `broker/tests/ibkr_reconcile.rs::test_orphan_order_detection`
- Assertion: `IBKRBroker::reconcile_state()` returns `ReconcileResult::Blocked` when an orphan order exists, preventing further order submission — see `broker/tests/ibkr_reconcile.rs::test_submission_blocked_on_orphan`

#### Phase 4: Reconnect Drill Test

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
- `cargo test -p nanobook-broker --test ibkr_f6_reconnect_drill` passes
- Assertion: after `MockTws::simulate_disconnect()` + `MockTws::simulate_reconnect()`, no duplicate orders are submitted — `broker/tests/ibkr_f6_reconnect_drill.rs::test_no_double_submit_on_reconnect`
- Assertion: `reconcile_duration_ms < 30_000` recorded in test output for `broker/tests/ibkr_f6_reconnect_drill.rs::test_reconnect_within_30s`

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

When the rebalancer process crashes (SIGKILL) mid-rebalance, the restart must read the audit log, reconstruct state at the failure point, and complete or roll back the rebalance cleanly — without double-submitting orders.

**Test**: Kill at every audit-log checkpoint, assert correct final state.

**Status:** ✅ DONE (commits 36aca43..f0dca02, closed 2026-05-13)

Implementation delivered across 5 commits:
- Phase 1 (36aca43): Audit log checkpoints — see `rebalancer/src/audit.rs` `Checkpoint` enum + `log_checkpoint()`
- Phase 2 (aa30eb8): State reconstruction — see `rebalancer/src/recovery.rs` `reconstruct_state()`
- Phase 3 (5728882): Broker state query — see `Broker::open_orders()` + `compare_broker_state()`
- Phase 4 (0b8f17b): `rebalancer recover` subcommand with `--dry-run`
- Phase 5 (f0dca02): `rebalancer/tests/recovery_integration.rs` — 7 tests, all checkpoints covered

**Known gap (tracked as bd-2tu):** `run_recover()` does not yet invoke `compare_broker_state()`; only prints reconstructed state + guidance.

**Verification:**
- `cargo test -p nanobook-rebalancer --test recovery_integration` → 7 passing
- `cargo test -p nanobook-rebalancer` → 92 passing total

---

## bd-2ln: v0.13 F-bin1 - Binance Idempotency Proof

### Problem Statement

Mirror v0.13 F7 (cron double-fire idempotency) for Binance broker adapter. The rebalancer may be triggered twice (e.g., cron misconfiguration, manual re-run). The adapter must detect double-fire via sequence-number/audit-log check and ensure exactly one set of orders is submitted to Binance.

**Scope**: Lighter than IBKR full 9 modes — Codex review consensus was "at least 2" for Binance.

### Deepened Implementation Plan

#### Phase 1: Binance Order Cache

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
- `cargo test -p nanobook-broker --test binance_order_cache` passes
- Assertion: `BinanceOrderCache` round-trips through serde: serialize to JSON, deserialize, compare — `broker/tests/binance_order_cache.rs::test_cache_serde_roundtrip`
- Assertion: cache is persisted to disk and reloaded on `BinanceBroker::new()` — `broker/tests/binance_order_cache.rs::test_cache_persistence_across_restart`

#### Phase 2: Binance Client Order IDs

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
- `cargo test -p nanobook-broker --test binance_idempotency` passes
- Assertion: second `submit_order()` call with same `client_order_id` returns `Err(BrokerError::DuplicateOrder)` — `broker/tests/binance_idempotency.rs::test_duplicate_client_order_id_rejected`
- Assertion: `MockBinance` contains exactly one order after two submissions with same `client_order_id` — `broker/tests/binance_idempotency.rs::test_mock_has_single_order`

#### Phase 3: Audit Log Integration

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
- `cargo test -p nanobook-broker --test binance_audit_idempotency` passes
- Assertion: audit log contains exactly one `OrderSubmitted` entry for sequence number N after two `submit_order()` calls — `broker/tests/binance_audit_idempotency.rs::test_audit_log_single_entry_per_sequence`
- Assertion: second submission with duplicate sequence N is rejected before reaching `MockBinance` — `broker/tests/binance_audit_idempotency.rs::test_double_fire_blocked_at_audit_check`

#### Phase 4: Binance Mock for Failure Injection

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
- `cargo test -p nanobook-broker --test binance_mock` passes
- Assertion: `MockBinance::submit_order()` returns the expected `OrderState` for each injected failure mode — `broker/tests/binance_mock.rs::test_mock_failure_injection`
- Assertion: `MockBinance` deduplicates by `client_order_id` consistently — `broker/tests/binance_mock.rs::test_mock_client_order_id_dedup`

#### Phase 5: F-bin1 Integration Test

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
- `cargo test -p nanobook-broker --test binance_f_bin1_idempotency` passes
- Assertion: after two invocations of the rebalancer with the same window/sequence, `MockBinance` contains exactly one order per symbol — `broker/tests/binance_f_bin1_idempotency.rs::test_f_bin1_end_to_end`
- Assertion: audit log contains one `IdempotencyRejection` event for the second run — `broker/tests/binance_f_bin1_idempotency.rs::test_f_bin1_audit_log_contains_rejection`

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

#### Phase 1: Binance WebSocket Implementation

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
- `cargo test -p nanobook-broker --test binance_websocket` passes
- Assertion: `BinanceWebSocket::connect()` successfully subscribes to user data stream and receives at least one `AccountUpdate` message from `MockBinanceWs` — `broker/tests/binance_websocket.rs::test_user_data_stream_subscription`
- Assertion: execution report messages are parsed into `OrderState` variants without panic — `broker/tests/binance_websocket.rs::test_execution_report_parsing`

#### Phase 2: Disconnect Detection

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
- `cargo test -p nanobook-broker --test binance_reconnect` passes
- Assertion: `BinanceBroker::is_connected()` returns `false` within 10s of `MockBinanceWs::simulate_disconnect()` — `broker/tests/binance_reconnect.rs::test_ws_heartbeat_timeout`
- Assertion: after auto-reconnect, `BinanceBroker::is_connected()` returns `true` and data stream resumes — `broker/tests/binance_reconnect.rs::test_auto_reconnect_restores_stream`

#### Phase 3: Account Info Query

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
- `cargo test -p nanobook-broker --test binance_account_reconcile` passes
- Assertion: `BinanceBroker::account_info()` returns balances matching `MockBinance` state — `broker/tests/binance_account_reconcile.rs::test_account_info_parsing`
- Assertion: after simulated disconnect, `BinanceBroker::reconcile_state()` detects orphan orders from `MockBinance` — `broker/tests/binance_account_reconcile.rs::test_orphan_order_detected_on_reconcile`

#### Phase 4: WebSocket + REST Fallback

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
- `cargo test -p nanobook-broker --test binance_rest_fallback` passes
- Assertion: after `MockBinanceWs::simulate_disconnect()`, `BinanceBroker` switches to REST polling mode and `is_using_rest_fallback()` returns `true` — `broker/tests/binance_rest_fallback.rs::test_auto_fallback_to_rest`
- Assertion: when WebSocket recovers, broker switches back and `is_using_rest_fallback()` returns `false` — `broker/tests/binance_rest_fallback.rs::test_fallback_reverts_on_ws_recovery`

#### Phase 5: F-bin2 Integration Test

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
- `cargo test -p nanobook-broker --test binance_f_bin2_reconnect_drill` passes
- Assertion: after `MockBinanceWs::simulate_disconnect()` + reconnect, no duplicate orders in `MockBinance` — `broker/tests/binance_f_bin2_reconnect_drill.rs::test_no_double_submit_on_reconnect`
- Assertion: `reconcile_duration_ms < 30_000` in test output — `broker/tests/binance_f_bin2_reconnect_drill.rs::test_reconnect_within_30s`

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
