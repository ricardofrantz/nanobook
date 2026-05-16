# Phase 1.6C Monitoring Requirements

Monitor Phase 1.6C at the operation-pair level. Every intent should normally have a matching result in the same run unless the process crashes.

## Required metrics

- `rebalancer.audit.write_errors_total{operation}` — must remain zero.
- `rebalancer.write_ahead.incomplete_intents_total{operation}` — alert on order or cancel immediately; warn on read-only operations.
- `rebalancer.broker.operation_latency_ms{operation}` — account summary, positions, quotes, submit, cancel.
- `rebalancer.broker.operation_failures_total{operation,error_class}` — transient vs permanent.
- `rebalancer.recovery.manual_review_total{reason}` — must be zero during normal rollout.
- `rebalancer.recovery.reconciled_intents_total` — expected only during crash drills.
- `rebalancer.audit.validation_failures_total` — alert on any live-run validation failure.
- `rebalancer.run.duration_ms` — alert if p95 regresses by more than 10%.
- `rebalancer.audit.bytes_written_per_run` — watch for unexpected growth.

## Intent/result ratios

Expected steady-state ratios:

- `account_summary_intent : account_summary_result = 1:1`
- `positions_intent : positions_result = 1:1`
- `quotes_intent : quotes_result = 1:1`
- `order_intent : (order_submitted + order_failed) = 1:1`
- `cancel_intent : cancel_result = 1:1`

An unmatched cancel intent is severity 1. An unmatched order intent is severity 2 unless broker reconciliation proves no order exists. Unmatched read-only intents are severity 3 unless they repeat.

## Rollout dashboard

During rollout, display:

1. Last 20 rebalance runs and final checkpoint.
2. Per-operation intent/result counts.
3. Broker operation latency p50/p95/p99.
4. Audit write latency and error count.
5. Recovery action distribution (`Restart`, `Resume`, `ManualReview`, `Rollback`).
