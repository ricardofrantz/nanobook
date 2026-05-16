# Phase 2.1 Graceful Shutdown

## Behavior

The rebalancer installs a SIGTERM handler at run start. The handler only sets an atomic shutdown flag, so it is safe to invoke while broker calls or audit writes are in progress.

During execution the runner checks the flag before each order and again after each order returns:

1. If SIGTERM arrives before an order starts, no new broker order is submitted.
2. If SIGTERM arrives during an order submission, the runner waits for that broker call to return.
3. If the current order returns as a partial fill, the runner attempts to cancel the remaining broker-side order before exiting.
4. Remaining queued orders are skipped.
5. The audit log receives `kill_completed` with `method=graceful`, `orders_cancelled_count`, and `duration_seconds`.

## Rollback criteria

Disable or revert graceful shutdown if any of these appear in staging or production:

- SIGTERM no longer stops the process after the current broker call.
- `kill_completed` is missing after graceful shutdown.
- Partial-fill cancellation fails repeatedly and leaves broker orders open.
- Shutdown latency exceeds the configured broker order timeout plus one cancellation attempt.
- Signal handling causes startup failures on supported Unix environments.

## Monitoring requirements

Track these operational signals:

- Count of `kill_completed` events by `method`.
- `duration_seconds` p95 for graceful shutdowns.
- `orders_cancelled_count` distribution.
- Any `cancel_result` with `success=false` after `cancellation_reason=graceful_shutdown_partial_fill`.
- Kill command dangling-order verification failures.

Expected steady state: graceful shutdowns are rare, `kill_completed` is always present after SIGTERM, and cancellation failures are zero.

## Incident response

If graceful shutdown does not complete safely:

1. Query IBKR/paper broker directly for open orders.
2. Cancel unexpected open orders manually or via broker tooling.
3. Inspect audit log around SIGTERM for `order_submitted`, `order_filled`, `cancel_intent`, `cancel_result`, and `kill_completed`.
4. If `kill_completed` is absent, treat the run as interrupted and use crash recovery/manual review.
5. If `cancel_result.success=false`, treat the broker state as safety-critical and keep the run in manual review until reconciled.
6. Patch or revert graceful shutdown before re-enabling unattended production runs.
