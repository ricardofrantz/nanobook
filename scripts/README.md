# Scripts

This directory contains utility scripts for nanobook operations.

## sanitize-audit.py

Publishable PII scrubber for nanobook audit logs.

### Purpose

Removes personally identifiable information (PII) from audit logs while preserving audit-log invariants needed for analysis:
- Sequence numbers (for cron mode idempotency)
- Timestamps (for clock skew detection)
- Order math (shares, prices, commission)
- Fill sequence (for reconciliation)

### PII Fields Scrubbed

- `account` - Account IDs in `run_started` events
- `ibkr_id` - Order IDs in `order_submitted` and `order_filled` events

### Usage

```bash
# Scrub an audit log (in-place)
python3 scripts/sanitize-audit.py audit.jsonl

# Scrub to a separate file
python3 scripts/sanitize-audit.py audit.jsonl audit_scrubbed.jsonl

# Check mode: verify no PII remains (exit code 1 if PII found)
python3 scripts/sanitize-audit.py --check audit.jsonl
```

### Idempotency

The script is idempotent — safe to run multiple times. Running it on an already-scrubbed file will report 0 lines with PII.

### Placeholder Values

- Account IDs: `"ACCOUNT_REDACTED"`
- Order IDs: `999999`

These placeholders are chosen to be clearly non-identifiable while preserving the data structure.
