# Warm Restart Guide

This guide documents the audit-log → state reconstruction protocol for operators. It explains what an operator should do after a crash: how to read the audit log, how to confirm position state matches IBKR's view, and when to manually intervene.

## Overview

### What is Warm Restart?

Warm restart is the process of recovering from a crash (process crash or TWS restart) by reconstructing the rebalancer's state from the audit log. Instead of starting from scratch, the rebalancer reads the audit log to determine:

- Where the crash occurred (which checkpoint was last reached)
- What orders were submitted and their fill status
- What positions were held at the time of the crash
- Whether the run completed successfully

Based on this reconstructed state, the system recommends a recovery action: `Restart`, `Resume`, `ManualReview`, or `Rollback`.

### When is Warm Restart Needed?

Warm restart is needed in two scenarios:

1. **Process crash**: The rebalancer process crashes during execution (e.g., due to a panic, OOM, or signal). This is tested by failure mode F9.
2. **TWS restart drill**: The IBKR Trader Workstation (TWS) or Gateway is restarted during order execution. This is tested by failure mode F6.

In both cases, the audit log provides a complete record of what happened before the crash, enabling safe recovery.

### How It Uses the Audit Log

The audit log is a JSON Lines (`.jsonl`) file that records every significant event during a rebalance run. Each line is a JSON object with:

- `event`: The event name (e.g., `run_started`, `order_submitted`)
- `ts`: Timestamp in UTC
- `sequence_number`: Monotonic sequence number for ordering
- `checkpoint`: Optional checkpoint type for crash recovery
- `data`: Event-specific data (positions, orders, etc.)

The recovery system parses the audit log, extracts checkpoint events, and reconstructs the state at the time of the last checkpoint.

## Audit Log Structure

### Checkpoint Events

The following checkpoints mark progress through a rebalance run:

| Checkpoint | Event Name | Description |
|------------|------------|-------------|
| `RunStarted` | `run_started` | Rebalance run begins |
| `PositionsFetched` | `positions_fetched` | Current positions retrieved from broker |
| `DiffComputed` | `diff_computed` | Rebalance diff computed (orders planned) |
| `RiskCheckPassed` | `risk_check_passed` | Risk checks passed (position limits, leverage, short exposure) |
| `OrderSubmitted` | `order_submitted` | Individual order submitted to IBKR |
| `OrderFilled` | `order_filled` | Individual order filled |
| `RunCompleted` | `run_completed` | Rebalance run completes (success or failure) |

### Sequence Numbers

Each checkpoint includes a monotonic `sequence_number` that:

- Ensures events are ordered correctly
- Detects missing or corrupted checkpoints
- Supports idempotency in cron mode (prevents double-firing the same window)

Sequence numbers must be strictly increasing. If the audit log has gaps or non-monotonic sequence numbers, recovery will fail with a validation error.

### How to Read the Audit Log

You can read the audit log using three methods:

#### 1. Using `cat` (raw JSONL)

```bash
cat audit.jsonl
```

Output example:
```json
{"event":"run_started","ts":"2026-02-08T15:30:00Z","sequence_number":1,"checkpoint":"RunStarted","data":{"target_file":"target.json","account":"U1234567"}}
{"event":"positions_fetched","ts":"2026-02-08T15:30:05Z","sequence_number":2,"checkpoint":"PositionsFetched","data":{"positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":1000000.0}}
```

#### 2. Using `jq` (formatted JSON)

```bash
cat audit.jsonl | jq
```

Output example:
```json
{
  "event": "run_started",
  "ts": "2026-02-08T15:30:00Z",
  "sequence_number": 1,
  "checkpoint": "RunStarted",
  "data": {
    "target_file": "target.json",
    "account": "U1234567"
  }
}
```

#### 3. Using `rebalancer recover` (parsed state)

```bash
rebalancer recover --target target.json
```

This command parses the audit log, reconstructs the state, and displays it in a human-readable format (see "Using rebalancer --recover" below).

## Recovery Actions

Based on the reconstructed state, the system recommends one of four recovery actions:

### Restart

**When to use:** Safe to restart the entire rebalance from the beginning.

**Conditions:**
- Run completed successfully (all orders filled)
- No orders were submitted before the crash
- All orders were filled before the crash

**Action:** Run `rebalancer run target.json` to start fresh. No manual intervention needed.

**Example:**
```
Recovery Action: Restart
Safe to restart the entire rebalance from the beginning.
No orders were submitted or all orders were filled.
Run: rebalancer run target.json
```

### Resume

**When to use:** Resume from the last checkpoint.

**Conditions:**
- Some orders were submitted but all are now filled
- State is consistent between audit log and broker

**Action:** Run `rebalancer run target.json` to continue. Manual review of broker state recommended before proceeding.

**Example:**
```
Recovery Action: Resume
Resume from the last checkpoint.
Some orders may have been submitted but not filled.
Manual review of broker state recommended before proceeding.
```

### ManualReview

**When to use:** Requires operator intervention to review state and decide on action.

**Conditions:**
- Crash occurred after order submission but before fills
- Unfilled orders exist in the reconstructed state
- Discrepancies detected between broker and reconstructed state

**Action:**
1. Verify IBKR TWS for open orders and positions
2. Check if orders were filled despite the crash
3. Decide whether to cancel orphan orders, wait for fills, or restart
4. Take appropriate action based on broker state

**Example:**
```
Recovery Action: ManualReview
Manual review required.
The crash occurred at an ambiguous point.
Please review broker state and decide on the appropriate action.
IMPORTANT: Verify IBKR TWS for open orders and positions before proceeding.
```

### Rollback

**When to use:** Cancel orphan orders and restart.

**Conditions:**
- Orders were submitted but may be in an unknown state
- Broker state does not match reconstructed state
- Orphan orders detected (orders in broker but not in audit log)

**Action:**
1. Cancel orphan orders via TWS or API
2. Verify all orders are cancelled
3. Run `rebalancer run target.json` to restart

**Example:**
```
Recovery Action: Rollback
Rollback recommended.
Orders were submitted but may be in an unknown state.
Cancel orphan orders via TWS before restarting.
```

## Using `rebalancer --recover`

### Command Syntax

```bash
rebalancer recover --target <target.json> [--dry-run]
```

- `--target <target.json>`: Path to the target specification file (required)
- `--dry-run`: Show recovery plan without executing (optional)

### Output Format

The command outputs three sections:

1. **Recovered State**: Last checkpoint, sequence number, timestamp, positions, orders, equity, run completion status
2. **Recovery Action**: Recommended action (Restart, Resume, ManualReview, Rollback)
3. **Broker State Comparison**: Discrepancies between broker and reconstructed state (if broker connection available)

Example output:
```
Recovering from crash using audit log: audit.jsonl

=== Recovered State ===
Checkpoint: OrderSubmitted
Sequence Number: 4
Timestamp: 2026-02-08T15:30:20Z
Positions: 1
  - AAPL: 100 shares @ $150.00
Orders: 1
  - AAPL: buy 50 @ $160.00 (submitted: true, filled: false)
Equity: $10000.00
Run Completed: false

=== Recovery Action ===
ManualReview

=== Broker State Comparison ===
Broker state matches reconstructed state.

=== Recovery Guidance ===
Manual review required.
The crash occurred at an ambiguous point.
Please review broker state and decide on the appropriate action.
IMPORTANT: Verify IBKR TWS for open orders and positions before proceeding.
```

### Dry-Run Mode

Use `--dry-run` to preview the recovery action without taking any action:

```bash
rebalancer recover --target target.json --dry-run
```

This is useful for:
- Understanding the crash point before deciding on action
- Verifying that the audit log is not corrupted
- Checking broker state comparison without committing to a recovery action

In dry-run mode, the command will exit with an error if `ManualReview` is required, but will not execute any recovery actions.

## Broker State Verification

### Manual Verification via TWS GUI

After a crash, manually verify IBKR state using the TWS GUI:

1. **Open Orders**: Check the "Orders" tab for any open orders
   - Note order IDs, symbols, quantities, and statuses
   - Compare with the audit log's `order_submitted` events

2. **Positions**: Check the "Portfolio" tab for current positions
   - Note symbols, quantities, and average costs
   - Compare with the audit log's `positions_fetched` event

3. **Account Summary**: Check the "Account" tab for equity and cash
   - Verify equity matches the audit log's equity value
   - Check for any unexpected changes

### Manual Verification via API

If you have API access, you can verify broker state programmatically:

```bash
# Show current positions
rebalancer positions

# Check IBKR connection
rebalancer status
```

### Using `rebalancer --recover` with Broker Connection

The `rebalancer recover` command now supports broker state comparison (via bd-2tu). When you run:

```bash
rebalancer recover --target target.json
```

The system will:
1. Attempt to connect to IBKR
2. Fetch current positions and open orders from the broker
3. Compare broker state with reconstructed state from the audit log
4. Generate a discrepancy report

If the broker connection fails, the command will proceed with recovery based on the audit log only, but will warn you to manually verify broker state.

### Interpreting Discrepancy Reports

The discrepancy report identifies mismatches between broker state and reconstructed state:

#### Orphan Orders

**Definition:** Order exists in broker but not in reconstructed state.

**Example:**
```
Orphan order: ID 12345 (symbol: UNKNOWN, status: Submitted)
```

**Action:** Cancel the orphan order via TWS or API before restarting.

**Cause:** Order was submitted but the audit log entry was lost (e.g., crash before fsync).

#### Missing Orders

**Definition:** Order exists in reconstructed state but not in broker open orders.

**Example:**
```
Missing order: AAPL (expected: Submitted but not filled)
```

**Action:** Check if the order was filled despite the crash. If filled, safe to restart. If not, resubmit the order.

**Cause:** Order was submitted but not recorded in broker (e.g., network issue before submission confirmation).

#### Order Status Mismatch

**Definition:** Order status differs between broker and reconstructed state.

**Example:**
```
Order status mismatch for AAPL: broker=Filled, expected=Submitted
```

**Action:** Trust the broker state. If the order is filled, safe to restart.

**Cause:** Order was filled after the crash but before recovery.

#### Position Mismatch

**Definition:** Position quantity differs between broker and reconstructed state.

**Example:**
```
Position mismatch for AAPL: broker=150, expected=100
```

**Action:** Reconcile positions. If the difference is due to filled orders, safe to restart. Otherwise, investigate.

**Cause:** Orders were filled after the crash, or positions were modified outside the rebalancer.

## Worked Examples

### Example 1: Crash Before Order Submission

**Scenario:** The rebalancer crashes after fetching positions but before submitting any orders.

**Audit log excerpt:**
```json
{"event":"run_started","ts":"2026-02-08T15:30:00Z","sequence_number":1,"checkpoint":"RunStarted","data":{"target_file":"target.json","account":"U1234567"}}
{"event":"positions_fetched","ts":"2026-02-08T15:30:05Z","sequence_number":2,"checkpoint":"PositionsFetched","data":{"positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":1000000.0}}
```

**Recovery action:** `Restart`

**Analysis:**
- Last checkpoint: `PositionsFetched`
- No orders submitted (orders list is empty)
- Safe to restart from beginning

**Action:**
```bash
rebalancer run target.json
```

**No manual intervention needed.**

---

### Example 2: Crash After Order Submission, Before Fill

**Scenario:** The rebalancer crashes after submitting an order but before receiving fill confirmation.

**Audit log excerpt:**
```json
{"event":"run_started","ts":"2026-02-08T15:30:00Z","sequence_number":1,"checkpoint":"RunStarted","data":{"target_file":"target.json","account":"U1234567"}}
{"event":"positions_fetched","ts":"2026-02-08T15:30:05Z","sequence_number":2,"checkpoint":"PositionsFetched","data":{"positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":1000000.0}}
{"event":"diff_computed","ts":"2026-02-08T15:30:10Z","sequence_number":3,"checkpoint":"DiffComputed","data":{"orders":[{"symbol":"AAPL","action":"Buy","shares":50,"limit":160.0}]}}
{"event":"order_submitted","ts":"2026-02-08T15:30:15Z","sequence_number":4,"checkpoint":"OrderSubmitted","data":{"symbol":"AAPL","action":"Buy","ibkr_id":12345}}
```

**Recovery action:** `ManualReview`

**Analysis:**
- Last checkpoint: `OrderSubmitted`
- Order submitted but not filled (unfilled order exists)
- Ambiguous state: order may have filled after the crash

**Action:**
1. Check IBKR TWS for order status (order ID 12345)
2. If filled: safe to restart
3. If still open: cancel or wait for fill
4. After resolving, run `rebalancer run target.json`

**Manual intervention required.**

---

### Example 3: TWS Restart During Order Execution

**Scenario:** IBKR TWS is restarted during order execution, causing a disconnection.

**Audit log excerpt:**
```json
{"event":"run_started","ts":"2026-02-08T15:30:00Z","sequence_number":1,"checkpoint":"RunStarted","data":{"target_file":"target.json","account":"U1234567"}}
{"event":"positions_fetched","ts":"2026-02-08T15:30:05Z","sequence_number":2,"checkpoint":"PositionsFetched","data":{"positions":[{"symbol":"AAPL","qty":100,"avg_cost":150.0}],"equity":1000000.0}}
{"event":"diff_computed","ts":"2026-02-08T15:30:10Z","sequence_number":3,"checkpoint":"DiffComputed","data":{"orders":[{"symbol":"AAPL","action":"Buy","shares":50,"limit":160.0}]}}
{"event":"order_submitted","ts":"2026-02-08T15:30:15Z","sequence_number":4,"checkpoint":"OrderSubmitted","data":{"symbol":"AAPL","action":"Buy","ibkr_id":12345}}
{"event":"connection_lost","ts":"2026-02-08T15:30:20Z","data":{"reason":"TWS restart"}}
{"event":"connection_restored","ts":"2026-02-08T15:31:00Z","data":{"reconnected":true}}
```

**Recovery action:** Check broker state via `--recover`

**Analysis:**
- Last checkpoint: `OrderSubmitted`
- Disconnection occurred after order submission
- Order may have filled during the disconnection

**Action:**
1. Run `rebalancer recover --target target.json` to compare broker state
2. Review discrepancy report:
   - If order filled: safe to restart
   - If order still open: cancel or wait for fill
   - If orphan orders detected: cancel before restarting
3. After reconciliation, run `rebalancer run target.json` or take manual action

**Broker state comparison is critical in this scenario.**

---

## Troubleshooting

### Audit Log Corruption

**Symptoms:**
- `rebalancer recover` fails with "Corrupted audit log at line N: invalid JSON"
- Checkpoint validation fails with "Checkpoint sequence not monotonic"

**Causes:**
- Disk failure or filesystem corruption
- Process crash during write (partial write)
- Manual editing of audit log

**Resolution:**
1. Identify the corrupted line number from the error message
2. If the corruption is at the end of the file, truncate to the last valid line
3. If the corruption is in the middle, restore from backup if available
4. If no backup available, manual reconstruction may be required (see "When to Escalate")

**Prevention:**
- Use filesystems with journaling or copy-on-write (e.g., ZFS, btrfs)
- Monitor disk health with SMART tools
- Backup audit logs regularly (see "Best Practices")

### Broker Connection Failures

**Symptoms:**
- `rebalancer recover` shows "Failed to connect to IBKR"
- Warning: "Proceeding with recovery without broker state comparison"

**Causes:**
- TWS or Gateway not running
- Network connectivity issues
- Incorrect host/port configuration
- API client ID conflict

**Resolution:**
1. Verify TWS or Gateway is running
2. Check `config.toml` connection settings (host, port, client_id)
3. Test connection with `rebalancer status`
4. If connection fails, proceed with audit log only, but manually verify broker state via TWS GUI

**Prevention:**
- Monitor TWS/Gateway health with external tools
- Use redundant network connections
- Document recovery procedures (see "Best Practices")

### Discrepancy Resolution

**Symptoms:**
- Discrepancy report shows orphan orders, missing orders, or position mismatches
- Recovery action is `ManualReview` or `Rollback`

**Resolution:**
1. **Orphan orders**: Cancel via TWS or API before restarting
2. **Missing orders**: Check if filled; if not, resubmit after restart
3. **Position mismatches**: Reconcile positions; investigate unexpected changes
4. **Multiple discrepancies**: Escalate (see "When to Escalate")

**Prevention:**
- Use `rebalancer reconcile target.json` regularly to detect drift
- Monitor audit logs for unexpected events
- Implement automated alerts for discrepancies

### When to Escalate

Escalate to engineering or senior operators if:

- Audit log is corrupted and no backup is available
- Multiple discrepancies exist that cannot be easily reconciled
- Broker state is inconsistent across multiple checks
- Recovery action is unclear after following this guide
- Unexpected behavior not covered in this guide

**Escalation information to collect:**
- Audit log excerpt (last 20 lines)
- `rebalancer recover --target target.json --dry-run` output
- TWS screenshots of open orders and positions
- Broker state comparison output
- System logs (rebalancer, TWS, OS)

## Best Practices

### Regular Audit Log Review

- Review audit logs after each rebalance run
- Check for unexpected events or gaps in sequence numbers
- Verify that `run_completed` is present for successful runs
- Archive old audit logs for historical analysis

### Backup Audit Logs

- Back up audit logs to a separate filesystem or cloud storage
- Use version control for audit logs if appropriate (ensure sensitive data is redacted)
- Retain audit logs for at least the regulatory retention period
- Test restore procedures regularly

### Document Recovery Actions

- Keep a log of all recovery actions taken
- Document the reason for each recovery action
- Note any discrepancies found and how they were resolved
- Share lessons learned with the team

### Test Recovery Procedure

- Practice recovery procedures in a test environment
- Simulate different crash scenarios (process crash, TWS restart, network failure)
- Verify that `rebalancer recover` produces the expected output
- Ensure team members are familiar with the recovery process

### Monitoring and Alerts

- Set up alerts for rebalancer crashes (process exit, error logs)
- Monitor audit log for sequence number gaps or corruption
- Alert on discrepancies between broker and reconstructed state
- Track recovery frequency and patterns

### Security Considerations

- Audit logs contain sensitive information (positions, orders, equity)
- On Unix, audit files are created with mode `0o600` (owner read/write only)
- On Windows, set ACLs manually on shared systems
- Restrict access to audit logs to authorized personnel only
- Redact sensitive data before sharing audit logs externally
