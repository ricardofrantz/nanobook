# Phase 1.6C Rollback Plan

## Rollback trigger

Disable `write_ahead_logging` and redeploy if any of these occur after enabling Phase 1.6C:

- Audit writes fail for account summary, positions, quotes, orders, or cancellation.
- Rebalance run failure rate increases by more than 5% from baseline.
- Account summary / positions / quote latency increases by more than 15%.
- Recovery reports incomplete cancellations or unresolved order intents.
- Operators see audit log validation failures on freshly produced live-run logs.

## Immediate rollback steps

1. Stop scheduled rebalancer runs.
2. Check broker UI for open orders and cancel manually if needed.
3. Redeploy without the Cargo feature: `--no-default-features` and without `--features write_ahead_logging`.
4. Run a dry-run rebalance and verify read-only broker calls work.
5. Restart scheduled runs only after audit/recovery state is understood.

## Data handling

Do not delete Phase 1.6C audit logs. Recovery and incident review need the intent/result pairs even when rolling back the feature. Older builds ignore unknown checkpoint event names during reconstruction where possible, and operators can inspect JSONL manually.

## Forward fix

Before re-enabling:

- Reproduce the failing operation against staging/paper broker.
- Run the feature-enabled and feature-disabled rebalancer test suites.
- Validate `tests/fixtures/complete_coverage.jsonl` and a freshly generated live-run audit log.
- Confirm monitoring has returned to baseline for at least one full rebalance window.
