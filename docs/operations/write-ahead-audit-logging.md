# Write-Ahead Audit Logging

Write-ahead audit logging records an intent before every broker operation that can affect live rebalancing state, then records the matching result after the operation completes. This gives recovery code and operators a durable trail for deciding what happened after a crash, disconnect, timeout, or manual interruption.

## Covered operations

| Operation | Intent event | Result event | Why it matters |
| --- | --- | --- | --- |
| Account summary fetch | `account_summary_intent` | `account_summary_result` | Captures sizing and risk inputs |
| Positions fetch | `positions_intent` | `positions_result` | Establishes current broker state |
| Quotes fetch | `quotes_intent` | `quotes_result` | Captures price inputs for sizing/orders |
| Order submission | `order_intent` | `order_submitted` or `order_failed` | Prevents duplicate or unknown submissions |
| Fill observation | — | `order_filled` | Captures execution outcome |
| Order cancellation | `cancel_intent` | `cancel_result` | Recovers kill-switch and cancel/fill race state |
| Run completion | — | `run_completed` | Marks that final reconciliation was recorded |

Read-only diagnostic commands may call the broker directly. The write-ahead model applies to the live execution path and shared cancellation paths where incomplete state can affect future trading decisions.

## Recovery semantics

- Incomplete account summary, positions, or quotes fetches are read-only and normally safe to restart.
- Incomplete order submissions require broker reconciliation before any resubmission.
- Incomplete cancellations require manual review because cancel/fill races are safety-critical.
- `run_completed` should be written only after final broker-observed state has been captured.

## Validator expectations

The audit validator should accept older logs where possible, but fresh live logs should follow this order:

1. `run_started`
2. optional `account_summary_intent` → `account_summary_result`
3. positions available through `positions_fetched` or `positions_intent` → `positions_result`
4. optional `quotes_intent` → `quotes_result`
5. `diff_computed`
6. `risk_check_passed`
7. zero or more order/cancel intent-result pairs
8. final reconciliation events
9. `run_completed`

Sequence numbers must be monotonic. Missing result events are allowed only as evidence of an interrupted operation; they should drive recovery, not be silently ignored.

## Monitoring signals

Recommended operational signals:

- audit write errors by operation;
- incomplete intents by operation;
- broker operation latency by operation;
- broker operation failures by operation and error class;
- recovery/manual-review decisions by reason;
- audit validation failures;
- audit bytes written per run.

Alert immediately on incomplete order or cancel intents in live operation. Read-only incomplete intents are lower severity but still worth investigating if repeated.

## Rollback and disablement

Disable write-ahead rollout or stop unattended runs if:

- audit writes fail;
- validation fails on freshly produced live logs;
- recovery reports incomplete cancellations or unresolved order intents;
- broker operation latency regresses enough to threaten execution safety;
- operators cannot determine state from the audit log.

Rollback should preserve existing audit files. Never delete or rewrite an audit log to make recovery appear clean.

## Related documentation

- [Warm restart](warm-restart.md)
- [Kill switch](kill-switch.md)
- [Rebalancer operations hardening](rebalancer-ops-hardening.md)
