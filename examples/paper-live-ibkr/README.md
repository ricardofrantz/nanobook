# Paper Trading IBKR Dry-Run

**Purpose:** Pre-soak rehearsal for v0.14 failure-injection hardening validation.

## Overview

This is a 1-week IBKR paper trading dry-run to validate v0.13's hardened plumbing with `--cron-mode`. The goal is to test the rebalancer's resilience to failure modes in a controlled paper trading environment before the full soak test.

## Run Details

**Status:** Setup phase (awaiting execution)

**Execution Window:** TBD (to be filled during execution)

**Universe:** TBD (to be filled during execution)

**Risk Limits:** TBD (to be filled during execution)

## Configuration

- **Mode:** IBKR paper account only (no real money)
- **Rebalancer:** v0.13 with hardened plumbing
- **Execution:** `--cron-mode` for idempotent cron-friendly execution
- **Risk Config:** `risk-config.toml` (see file for details)

## Audit Logs

Audit logs are stored in `audit/` directory. These logs contain:
- Run metadata (timestamps, sequence numbers)
- Order submission and fill events
- Risk limit checks
- Failure injection events (if any)

**Note:** Audit logs are NOT committed to git (see `.gitignore`). Use `scripts/sanitize-audit.py` to scrub PII before publishing.

## Runner Script

The `runner.sh` script is designed for cron execution:
```bash
# Manual execution
./runner.sh risk-config.toml

# Cron entry example (run every 30 minutes during market hours)
*/30 09:30-16:00 * * 1-5 /path/to/examples/paper-live-ibkr/runner.sh /path/to/examples/paper-live-ibkr/risk-config.toml
```

## Post-Run Analysis

After the dry-run completes:
1. Review audit logs for anomalies
2. Verify cron-mode idempotency (no duplicate orders)
3. Check failure injection handling
4. Sanitize audit logs before sharing: `python3 scripts/sanitize-audit.py audit/audit.jsonl`

## Notes

- This is scaffolding only - actual IBKR credentials and specific risk limits will be configured during execution
- All trading is paper-only - no real money at risk
- The dry-run validates plumbing robustness, not strategy performance
