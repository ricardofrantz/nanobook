# Phase 1.6C Complete Write-Ahead Coverage

Phase 1.6C extends the rebalancer write-ahead audit model from order submission to every broker operation that can influence a live rebalance decision or broker state.

## Covered operations

| Operation | Intent checkpoint | Result checkpoint | Production path |
| --- | --- | --- | --- |
| Account summary fetch | `account_summary_intent` | `account_summary_result` | Initial sizing/risk input and final reconciliation |
| Positions fetch | `positions_intent` | `positions_result` | Initial current state and final reconciliation |
| Quotes fetch | `quotes_intent` | `quotes_result` | Price input and final reconciliation |
| Order submission | `order_intent` | `order_submitted` or `order_failed` | Live rebalance execution |
| Order fill observation | — | `order_filled` | Live rebalance execution |
| Order cancellation | `cancel_intent` | `cancel_result` | Shared wrapper for kill-switch/future cancellation paths |

Diagnostic commands that do not execute a rebalance (`status`, `positions`, ad-hoc `reconcile`) still use direct read-only broker calls. Phase 1.6C coverage applies to the live rebalance execution path and the shared cancellation wrapper.

## Recovery behavior

- Incomplete account summary, positions, or quotes fetches are read-only and are safe to restart.
- Incomplete order submissions still trigger broker reconciliation before any resubmission.
- Incomplete cancellations require manual review because broker-side cancel/fill race state is safety-critical.
- `run_completed` is written after final post-execution reconciliation so the final checkpoint means all broker-observed state was captured.

## Validation

The checkpoint validator now enforces monotonic sequence numbers and flexible phased ordering:

1. `run_started`
2. Optional `account_summary_intent` → `account_summary_result`
3. Positions available via either legacy `positions_fetched` or new `positions_intent` → `positions_result`
4. Optional `quotes_intent` → `quotes_result`
5. `diff_computed`
6. `risk_check_passed`
7. `order_intent`
8. Optional submission/fill/completion checkpoints

This accepts Phase 1.6A/1.6B audit logs while validating Phase 1.6C complete coverage.
