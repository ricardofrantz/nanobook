# Phase 2 two-phase kill switch

The `guaranteed_kill_switch` build feature changes `rebalancer kill` from graceful-only termination to a two-phase safety workflow.

## Workflow

1. **Phase 1: graceful shutdown**
   - Read the runner PID from the PID file.
   - Append `kill_requested` with `method = "two_phase"`.
   - Append `kill_phase1_started` with the PID.
   - Send `SIGTERM` to the runner.
   - Wait for the configured timeout, defaulting to 30 seconds.
   - If the process exits, append `kill_phase1_completed`, verify the audit log has no dangling submitted orders, and append `kill_completed`.

2. **Phase 2: forceful broker cancellation**
   - Runs only when phase 1 times out or the final audit-log check still finds dangling orders.
   - Append `kill_phase2_started` with the PID.
   - Connect directly to IBKR using the configured connection settings, bypassing the stuck runner.
   - Query cancellable open orders, cancel each one, then verify with exponential backoff retry.
   - Append `kill_phase2_completed` with attempts, cancelled order IDs, remaining order IDs, and errors.
   - Append `kill_completed` with `method = "forced"` and the final counts.

Without `guaranteed_kill_switch`, `rebalancer kill` keeps the previous graceful-only behavior.

## Timeout configuration

Timeout precedence is:

1. CLI: `rebalancer kill --timeout-secs N`
2. Environment: `NANOBOOK_KILL_TIMEOUT_SECS=N`
3. Config: `[kill] timeout_secs = N`

The config default is 30 seconds. Zero is rejected by config validation; invalid or zero CLI/env values are ignored in favor of the next source.

Example config:

```toml
[kill]
timeout_secs = 30
```

## Audit events

Expected phase-1 success sequence:

```text
kill_requested(method=two_phase)
kill_phase1_started
kill_phase1_completed
kill_completed(method=graceful)
```

Expected phase-1 timeout / phase-2 success sequence:

```text
kill_requested(method=two_phase)
kill_phase1_started
kill_phase2_started
kill_phase2_completed
kill_completed(method=forced)
```

Expected phase-2 partial failure sequence has the same event names as phase-2 success, but `kill_phase2_completed.orders_remaining_count > 0`, non-empty `remaining_order_ids`, and `kill_completed.orders_remaining_count > 0`.

## Operational thresholds

- Phase-1 timeout: configurable, default 30s.
- Phase-2 broker connection plus cancellation should normally complete in 5-10s.
- Total kill time should normally stay below 40s with the default timeout.
- Treat total kill time above 60s as an operational rollback signal.

## Rollback criteria

Rebuild and redeploy without `guaranteed_kill_switch` if any of these happen repeatedly:

- Phase 1 cannot send `SIGTERM` because of permission or PID errors.
- Phase 2 broker connection failure rate exceeds 20%.
- Forceful cancellation failure rate exceeds 10%.
- Total kill time p95 exceeds 60s.
- Kill audit events cannot be written reliably.

Rollback preserves the previous graceful-only kill path.

## Monitoring

Track these production signals:

- Kill command success rate, target > 99%.
- Phase-1 success rate, target > 90%.
- Phase-2 success rate, target > 95%.
- Phase-1 duration, p95 below configured timeout.
- Phase-2 duration, p95 < 10s.
- Total kill duration, p95 < 40s with default timeout.
- Audit log write failures during kill, target zero.
- Remaining order IDs reported by `kill_phase2_completed`, target zero.

## Incident response

If two-phase kill reports remaining orders or cannot connect to the broker:

1. Check whether the rebalancer process is still running and whether the PID file is stale.
2. Verify IBKR connectivity from the host running the kill command.
3. Cancel remaining orders manually in the broker UI if any `remaining_order_ids` are present or if broker state is uncertain.
4. Preserve the audit log and application logs.
5. Investigate why phase 1 timed out or why broker cancellation failed.
6. Re-enable `guaranteed_kill_switch` only after the failure mode is reproduced and fixed in staging.
