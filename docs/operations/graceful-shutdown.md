# Graceful Shutdown

Graceful shutdown lets an operator or supervisor stop a running rebalance without interrupting a broker call in the middle of a critical section.

## Behavior

At run start, the rebalancer installs a SIGTERM handler. The handler only sets an atomic shutdown flag, so it is safe to trigger while broker calls or audit writes are in progress.

During execution, the runner checks the flag before each order and after each order returns:

1. If SIGTERM arrives before an order starts, no new broker order is submitted.
2. If SIGTERM arrives during an order submission, the runner waits for that broker call to return.
3. If the current order returns as a partial fill, the runner attempts to cancel the remaining broker-side quantity before exiting.
4. Remaining queued orders are skipped.
5. The audit log receives `kill_completed` with `method=graceful`, `orders_cancelled_count`, and `duration_seconds`.

## Expected operator workflow

1. Send SIGTERM through the service manager or kill workflow.
2. Wait for the current broker call plus one cancellation attempt to finish.
3. Confirm `kill_completed` appears in the audit log.
4. Check broker open orders if any cancellation failed or if `kill_completed` is missing.
5. Use warm restart/manual review for any ambiguous order state.

## Monitoring signals

Recommended signals:

- count of `kill_completed` events by method;
- graceful shutdown duration p95;
- distribution of `orders_cancelled_count`;
- cancellation failures after partial fills;
- dangling-order verification failures from kill commands.

Expected steady state: graceful shutdowns are rare, `kill_completed` is always present after SIGTERM, and cancellation failures are zero.

## Rollback criteria

Disable or revert graceful shutdown behavior if any of these appear repeatedly:

- SIGTERM no longer stops the process after the current broker call;
- `kill_completed` is missing after graceful shutdown;
- partial-fill cancellation leaves broker orders open;
- shutdown latency exceeds the configured broker order timeout plus one cancellation attempt;
- signal handling causes startup failures on supported Unix environments.

## Incident response

If graceful shutdown does not complete safely:

1. Query the broker directly for open orders.
2. Cancel unexpected open orders manually or through broker tooling.
3. Inspect audit events around SIGTERM, especially `order_submitted`, `order_filled`, `cancel_intent`, `cancel_result`, and `kill_completed`.
4. If `kill_completed` is absent, treat the run as interrupted and use warm restart.
5. If `cancel_result.success=false`, keep the run in manual review until broker state is reconciled.

## Related documentation

- [Kill switch](kill-switch.md)
- [Warm restart](warm-restart.md)
- [Write-ahead audit logging](write-ahead-audit-logging.md)
