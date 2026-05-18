# Rebalancer Operations Hardening

This document summarizes the operational failure modes that shaped the rebalancer hardening work. The goal is to make live or paper rebalancing fail safe: no duplicate submissions, no silent state drift, clear recovery guidance, and enough audit evidence for an operator to decide what happened.

## Scope

The hardening work covers broker adapters, the rebalancer execution path, audit logging, restart recovery, cron idempotency, and emergency stop behavior.

| Failure mode | Scenario | Defense |
| --- | --- | --- |
| Duplicate order-status callbacks | Broker repeats status/fill callbacks | Deduplicate callbacks by order/status/fill key before acting on them |
| Cancel reject race | Cancel request races with an in-flight fill | Treat cancel rejection as ambiguous until broker state is reconciled |
| Partial fill then disconnect | Order partially fills before connection loss | Reconnect, query broker state, and avoid automatic remainder resubmission |
| Stale market data | Rebalance uses quotes that are too old | Timestamp quotes and reject stale inputs before submitting orders |
| Clock skew | Host clock jumps forward/backward | Detect skew in audit logging and surface warnings |
| Broker restart | TWS/Gateway restarts during a run | Reconnect with backoff, query open orders/positions, and reconcile local state |
| Duplicate scheduled run | Cron/manual retry fires same window twice | Use sequence/window identifiers in the audit log to reject duplicates |
| Emergency stop | Operator needs to stop a running rebalance | Provide kill workflows that stop the runner and verify/cancel open orders |
| Process crash | Runner dies mid-rebalance | Reconstruct state from audit checkpoints and require broker reconciliation |

## Main design patterns

### Audit log as source of truth

The rebalancer writes append-only JSONL audit events with timestamps, sequence numbers, and operation checkpoints. The audit log is used for:

- duplicate scheduled-run detection;
- crash recovery and warm restart;
- operator review after ambiguous broker failures;
- regression fixtures for validation tests.

A successful live run should end with `run_completed` after final broker-observed state has been captured.

### Broker state wins over local assumptions

If local reconstructed state and broker state disagree, the broker state is treated as authoritative. The rebalancer should not submit more orders until open orders, fills, and positions are understood.

Discrepancies to look for:

- orphan orders present at the broker but absent from the audit log;
- expected orders missing from broker open orders;
- order status mismatch, especially submitted-vs-filled;
- position quantity or average-cost mismatch.

### Manual review for ambiguous states

The system deliberately avoids pretending ambiguous trading state is safe. When an order may have been submitted, cancelled, or filled without complete local evidence, recovery should stop in manual review and instruct the operator to inspect broker state.

### Deterministic failure-injection tests

The hardening work uses mock broker behavior to reproduce disconnects, duplicate callbacks, stale data, cancel races, and restart scenarios without depending on a live broker session. Real broker paper/live validation is still required before making public battle-tested claims.

## Operator guidance

After any abnormal run:

1. Preserve the audit JSONL exactly as produced.
2. Check broker UI/API for open orders, recent fills, positions, and account equity.
3. Run recovery in dry-run mode when available.
4. Compare recovered state against broker state.
5. Cancel unexpected open orders before restarting automation.
6. Keep scheduled runs disabled until state is reconciled.

## Developer guidance

When adding a broker operation that can affect trading state:

1. Add intent and result audit events before and after the operation.
2. Make failures explicit in typed errors; avoid panics in operational paths.
3. Include enough context in errors and audit data for manual reconciliation.
4. Add deterministic tests for duplicate, timeout, disconnect, and partial-success cases.
5. Update the operation docs when recovery behavior changes.

## Related documentation

- [Write-ahead audit logging](write-ahead-audit-logging.md)
- [Warm restart](warm-restart.md)
- [Graceful shutdown](graceful-shutdown.md)
- [Kill switch](kill-switch.md)
