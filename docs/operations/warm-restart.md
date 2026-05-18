# Warm Restart

Warm restart is the rebalancer recovery path for a process crash, host restart, broker disconnect, or TWS/Gateway restart. It reconstructs the last known run state from the audit log, compares that state with broker state, and recommends whether to restart, resume, roll back, or stop for manual review.

## When to use it

Use warm restart after any interrupted rebalance run, including:

- the process was killed or crashed before `run_completed`;
- TWS/Gateway restarted while orders were active;
- network loss occurred after an order was submitted;
- the audit log contains an intent event without the matching result event;
- broker positions/open orders do not match operator expectations.

## Audit log inputs

The recovery system reads JSONL audit events. Each event should include enough data to order events and identify the broker operation being recovered.

Common fields:

| Field | Purpose |
| --- | --- |
| `event` | Event name, such as `run_started`, `order_intent`, or `run_completed` |
| `ts` | UTC timestamp |
| `sequence_number` | Monotonic event ordering within a run |
| `checkpoint` | Optional checkpoint name for crash recovery |
| `data` | Event-specific payload, such as positions, target orders, or broker IDs |

Recovery requires sequence numbers to be monotonic. Gaps, duplicated sequence numbers, or malformed JSON should be treated as audit-log corruption and escalated to manual review.

## Recovery actions

| Action | Meaning | Operator response |
| --- | --- | --- |
| `Restart` | No broker-side state appears ambiguous | Start a new run after a normal sanity check |
| `Resume` | The last checkpoint can be continued safely | Confirm broker state first, then continue |
| `ManualReview` | Submitted/cancelled/filled state is ambiguous | Inspect broker state and decide manually |
| `Rollback` | Broker state contains orphan or unexpected orders | Cancel/resolve broker orders before restarting |

Broker state is authoritative. If the audit log says one thing and the broker says another, trust the broker and preserve both records for investigation.

## Recovery workflow

1. Stop scheduled rebalancer runs.
2. Preserve the audit log and application logs.
3. Inspect broker open orders, recent fills, positions, and account equity.
4. Run recovery in dry-run mode when available:

   ```bash
   rebalancer recover --target target.json --dry-run
   ```

5. Compare the recovered state with broker state.
6. If any order or position is ambiguous, keep automation disabled and resolve it manually.
7. Restart automation only after broker state and target state are reconciled.

## Broker-state checks

### Open orders

Look for orders that are:

- present at the broker but absent from the audit log;
- present in the audit log but missing from broker open orders;
- still submitted after the run should have completed;
- partially filled with remaining quantity outstanding.

Unexpected open orders should usually be cancelled before another rebalance run.

### Positions

Compare broker positions against the last audited positions and expected fills. A position mismatch may be normal if an order filled after the process crashed, but it must be explained before restarting.

### Account equity and cash

Confirm account equity/cash are consistent with fills and commissions. If equity changed outside the rebalancer, re-run sizing from fresh account state.

## Worked examples

### Crash before order submission

Last checkpoint is before any `order_intent` or `order_submitted` event. No broker order should exist.

Expected action: `Restart`, after confirming the broker has no unexpected open orders.

### Crash after order submission

Audit log includes `order_submitted`, but no fill or completion event.

Expected action: `ManualReview`. Check broker order status. If filled, update expectations from broker positions. If still open and not wanted, cancel it before restarting.

### Broker has an orphan order

Broker open orders include an order ID that the audit log does not know about.

Expected action: `Rollback` or `ManualReview`. Cancel or otherwise resolve the orphan order before another automated run.

### Completed run

Audit log ends with `run_completed`, final reconciliation succeeded, and broker state matches.

Expected action: no recovery needed. If the operator still suspects drift, start the next run from fresh positions and quotes.

## Related documentation

- [Write-ahead audit logging](write-ahead-audit-logging.md)
- [Rebalancer operations hardening](rebalancer-ops-hardening.md)
- [Kill switch](kill-switch.md)
