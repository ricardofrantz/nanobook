# Phase 1.6C Incident Response

## Severity

- **SEV1:** Incomplete `cancel_intent`, unknown broker cancellation state, or live open orders that cannot be reconciled.
- **SEV2:** Incomplete `order_intent`, duplicate broker order suspicion, or recovery cannot query broker.
- **SEV3:** Repeated incomplete read-only intents (`account_summary`, `positions`, `quotes`) or validation failures in dry-run/staging.

## First response

1. Pause scheduled rebalancer runs.
2. Preserve the audit JSONL exactly as found.
3. Inspect broker UI for open orders, recent fills, and cancellation status.
4. Run recovery in dry-run mode and record the recommended action.
5. If cancellation or order state is unknown, keep automation disabled until manually resolved.

## Incomplete account summary / positions / quotes

These are read-only. Usually safe response:

1. Verify broker connectivity.
2. Re-run in dry-run mode.
3. Compare the fresh broker state with the incomplete audit log.
4. Re-enable only after intent/result ratios return to 1:1.

## Incomplete order intent

1. Query broker open orders and recent fills.
2. Match by client order id where available, otherwise by symbol/side/quantity/time window.
3. If an order exists, mark the incident as reconciled and do not resubmit blindly.
4. If no order exists and broker history confirms no submission, restart the run.

## Incomplete cancellation

Cancellation is manual-review only:

1. Check whether the order filled, cancelled, or remains open.
2. If open and unwanted, cancel through broker UI.
3. If filled, update incident notes with fill quantity/price and run reconciliation.
4. Do not restart automation until the order state is known.

## Exit criteria

- Broker open orders match expected state.
- Recovery no longer recommends `ManualReview` for the incident log.
- A feature-enabled dry run completes with valid intent/result ratios.
- Monitoring has no audit write or validation errors for the next scheduled window.
