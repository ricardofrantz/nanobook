# v0.15 Paper-Live Soak Status

> This file is a live operator checklist. Do not mark a row complete unless the
> evidence column points to an audit excerpt, generated report, note, or commit.

## Current state

| Item | Status | Evidence |
| --- | --- | --- |
| Local preflight passes for selected config/target | pending | run `./preflight.sh my-config.toml my-target.json` |
| IBKR Gateway/TWS paper connection verified | pending | `rebalancer --config my-config.toml status` output |
| Dry-run on paper account completed | pending | log path + audit excerpt |
| First paper execution completed | pending | log path + audit excerpt |
| One-week pre-soak rehearsal completed | pending | daily rows below + weekly summary |
| 14 trading-day minimum soak completed | pending | daily rows below + generated report |
| Sanitized audit excerpt generated | pending | `sanitized-audit.jsonl` path/hash |
| HTML report generated from sanitized excerpt | pending | `report.html` path/hash |
| Paper-soak learnings filled from evidence | pending | `docs/solutions/paper-soak-learnings.md` |
| README battle-tested numbers updated | pending | README commit |
| Release/tag completed | pending | tag + changelog commit |

## Daily check-ins

| Date | Trading day # | Run attempted? | Orders / fills / cancels | Reconcile result | Incidents | Evidence | Decision |
| --- | ---: | --- | --- | --- | --- | --- | --- |
| _TBD_ | 1 | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ | continue / patch / restart |

## Weekly summaries

| Week | Trading days | Runs | Incidents | Manual interventions | Patches | Evidence | Decision |
| --- | ---: | ---: | ---: | ---: | --- | --- | --- |
| _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ | _TBD_ | continue / extend / release / restart |

## Stop / restart rules

Restart the soak from day 0 after any patch that changes production behavior.
Do not roll uncovered failures into a v0.15 release candidate silently.

Stop and patch before continuing if any of these happen:

- Duplicate order or ambiguous idempotency state.
- Broker open orders remain after graceful/forceful kill verification.
- Audit log corruption prevents reconstruction.
- Reconcile mismatch cannot be explained from broker state.
- IBKR paper reject or missing-data behavior requires code/config changes.
- Operator manually fixes state without an audit-backed note.

## Commands

```bash
# local validation only; does not contact IBKR
./preflight.sh my-config.toml my-target.json

# connection check; contacts IBKR but submits no orders
../../target/release/rebalancer --config my-config.toml status

# dry-run; no orders submitted
../../target/release/rebalancer --config my-config.toml run my-target.json --dry-run

# cron/idempotent run via wrapper
./runner.sh my-config.toml my-target.json

# sanitize and report after evidence exists
python3 ../../scripts/sanitize-audit.py audit/audit.jsonl > sanitized-audit.jsonl
python3 report.py sanitized-audit.jsonl report.html
```
