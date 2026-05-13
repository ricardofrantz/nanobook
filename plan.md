# v0.13 Status

## Quick Wins Completed ✅ (2026-05-13)

### bd-2tu: F9 follow-up - Wire compare_broker_state() into run_recover
- Modified run_recover() to accept Option<&dyn Broker> parameter
- Added broker state comparison logic after reconstructing state
- Updated Recover subcommand to create and connect broker
- Added integration test for broker state comparison
- Enhanced Rollback and ManualReview guidance with discrepancy notes
- Commit: 964901e

### bd-2pu: Write docs/ops/warm-restart.md
- Created comprehensive warm restart guide for operators
- Documented audit-log → state reconstruction protocol
- Included 3 worked examples with audit log excerpts
- Added broker state verification guidance
- Added troubleshooting and best practices sections
- Updated README.md with reference
- Commit: 8412071

### bd-2ia: Write docs/solutions/ops-hardening-learnings.md
- Documented all 9 IBKR failure modes (F1-F9)
- Created failure mode summary table
- Detailed analysis per failure mode (v0.10 vs v0.13)
- Cross-cutting learnings (audit log, reconciliation, graceful degradation)
- Architectural changes documentation
- Recommendations for v0.14, operators, and developers
- Created docs/solutions/README.md as index
- Commit: 0e41ef9

## Remaining v0.13 Blockers

### bd-2ln: v0.13 F-bin1 - Binance idempotency proof
- Status: OPEN
- Effort: 10-13 days
- Description: Mirror F7 for Binance - sequence-number/audit-log check for double-fire idempotency
- Dependencies: MockTws harness ✅
- 5 phases: Binance Order Cache, Client Order IDs, Audit Log Integration, Binance Mock, Integration Test

### bd-1pd: v0.13 F-bin2 - Binance reconnect drill
- Status: OPEN
- Effort: 11-15 days
- Description: Mirror F6 for Binance - WebSocket drop + reconnect, state reconciliation, 30s target
- Dependencies: MockTws harness ✅, F-bin1 (bd-2ln)
- 5 phases: Binance WebSocket, Disconnect Detection, Account Info Query, WebSocket + REST Fallback, Integration Test

## Recommendation

The Binance failure modes (F-bin1, F-bin2) are large efforts (21-28 days combined). Consider:

**Option A:** Ship v0.13 without Binance failure modes
- All 9 IBKR failure modes are complete
- Documentation is complete
- Binance modes can be v0.14 or a separate track
- Unblocks v0.13 release immediately

**Option B:** Complete Binance modes before v0.13 release
- Ensures parity between IBKR and Binance adapters
- Requires 21-28 additional days
- Delays v0.13 release

**Option C:** Split v0.13 into v0.13.0 (IBKR-only) and v0.13.1 (IBKR + Binance)
- Ships IBKR hardening sooner
- Binance modes follow in v0.13.1
- Maintains semantic versioning consistency

## Overview
Implement F6 (bd-w7x): TWS restart mid-position (reconnect drill). When TWS restarts while the rebalancer has open positions, the broker adapter must detect disconnect, reconnect, query IBKR state, reconcile against local state, and resume monitoring without double-submitting orders.

**Target**: Reconcile within 30s of TWS coming back up.

## Implementation Summary

### Phase 1: Disconnect Detection ✅
**Commit:** f80b046

Added connection state tracking and auto-reconnect logic to IBKR broker adapter.

**Changes:**
- Added `ReconnectFailed` error variant to `BrokerError`
- Added `ConnectionState` enum (Connected, Disconnected, Reconnecting)
- Added `connection_state` field to `IbkrBroker`
- Added `is_connected()` and `connection_state()` methods
- Added `reconnect_with_backoff()` method with exponential backoff (1s, 2s, 4s, 8s, 16s max)
- Max reconnect attempts: 5
- Created `ibkr_reconnect.rs` test suite with 8 tests

**Verification:** All 97 broker tests pass.

### Phase 2: Open Positions Query ✅
**Commit:** 333a923

Implemented IBKR open-positions endpoint query and local order cache.

**Changes:**
- Implemented `IbkrClient::open_orders()` using ibapi's `all_open_orders()` API
- Added `CachedOrder` struct and thread-safe order cache (Mutex<HashMap>)
- Implemented order cache methods: `cache_order()`, `update_cached_order_status()`, `get_cached_order()`, `clear_order_cache()`
- Wired order caching into `submit_order()` - orders cached after successful submission
- Updated `reconnect()` to clear order cache to avoid stale state
- Implemented `IbkrBroker::reconcile_state()` method
- Added local `DiscrepancyReport` and `Discrepancy` types (OrphanOrder, MissingOrder, OrderStatusMismatch, PositionMismatch)
- Created `ibkr_state_query.rs` test suite with 4 tests

**Verification:** All broker tests pass.

### Phase 3: State Reconciliation ✅
**Commit:** 89e6c04

Added reconciliation safety checks and broker state verification guidance.

**Changes:**
- Added `reconciliation_blocked` field to `IbkrBroker`
- Added `is_reconciliation_blocked()`, `block_reconciliation()`, `unblock_reconciliation()` methods
- Modified `submit_order()` to block when reconciliation is blocked
- Modified `reconcile_state()` to set block flag on discrepancies
- Added broker state verification guidance in `run_recover()`
- Created `ibkr_reconcile.rs` test suite with 7 tests

**Verification:** All 117 rebalancer tests pass.

### Phase 4: Reconnect Drill Test ✅
**Commit:** d94d146

Implemented end-to-end test of F6 failure mode with MockTws extensions and 30s target measurement.

**Changes:**
- Extended MockTws with `simulate_disconnect()`, `simulate_reconnect()`, `simulate_partial_fill()`
- Added state persistence across disconnect/reconnect
- Added `get_order()` and `all_orders()` getter methods
- Updated `F6ReconnectDrill` failure injection to use new methods
- Created `ibkr_f6_reconnect_drill.rs` integration test suite with 4 tests
- Added timing measurement utilities
- Tests verify: no double submit, 30s target, state persistence, orphan order detection

**Verification:** All F6 integration tests pass.

## Total Test Coverage

- Phase 1: 8 tests
- Phase 2: 4 tests
- Phase 3: 7 tests
- Phase 4: 4 tests
- **Total: 23 new tests**

All tests pass across the broker and rebalancer crates.

## Dependencies

- MockTws harness (bd-23o) - ✅ Complete
- IBKR broker adapter (existing)
- Audit log (existing)
- Recovery machinery from F9 (✅ Complete)

## Next Steps

F6 (bd-w7x) is now complete. This unblocks:
- bd-2pu (warm-restart docs)
- bd-1j2 (v0.13 release)

The rebalancer now supports TWS restart mid-position with:
- Automatic disconnect detection
- Exponential backoff reconnection
- State reconciliation against broker
- Safety checks to prevent double-submits
- End-to-end test coverage with 30s target verification
