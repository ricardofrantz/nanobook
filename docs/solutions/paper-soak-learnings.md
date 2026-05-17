# Paper Soak Learnings: v0.15

> Status: **pre-soak scaffold**. This document is intentionally not a success narrative yet.
> It becomes the final v0.15 learning document only after the IBKR paper-live soak
> has actually run and the sanitized audit excerpts/report are available.

## Purpose

The v0.15 paper-live soak is meant to answer one question honestly:

> Does nanobook's hardened rebalancer survive real IBKR paper-trading operations
> without an operator papering over broker, scheduler, market-data, or audit-log failures?

This document captures the operational incidents and surprises that happen during
that soak. It should be updated from the sanitized audit trail, the generated
`examples/paper-live-ibkr/report.html`, and any operator notes taken during daily
check-ins.

## Evidence sources

Use only reconstructable evidence:

- Sanitized audit JSONL excerpts from `examples/paper-live-ibkr/audit/`
- Generated report from `examples/paper-live-ibkr/report.py`
- Daily check-in notes written during the soak
- Weekly summaries written during the soak
- Any v0.13.x patch commits created because the soak exposed a failure

Do **not** fill this document from memory after the fact. If an incident is not in
one of the evidence sources above, mark it as uncertain instead of promoting it
to fact.

## Soak summary

| Field | Value |
| --- | --- |
| Start date | _TBD_ |
| End date | _TBD_ |
| Trading days completed | _TBD_ |
| Rebalance runs attempted | _TBD_ |
| Orders submitted | _TBD_ |
| Orders filled | _TBD_ |
| Reconcile checks | _TBD_ |
| Reconcile mismatches | _TBD_ |
| Operational incidents | _TBD_ |
| Manual interventions | _TBD_ |
| v0.13.x patches required | _TBD_ |
| Soak restarted from day 0? | _TBD_ |

## Incident log

Every incident belongs here, even if it auto-recovered and felt boring.

| Date/time | Category | What happened | Evidence | Recovery | Follow-up |
| --- | --- | --- | --- | --- | --- |
| _TBD_ | reconnect / reject / stale-data / slippage / clock / kill-switch / cron / other | _TBD_ | audit line(s), report section, or note path | automatic / manual / unknown | patch, config change, or no action |

### Incident categories to watch

- **Reconnects:** TWS/Gateway socket drops, reconnect latency, duplicated callbacks after reconnect.
- **Rejects:** IBKR order rejects, insufficient buying power, symbol/account constraints.
- **Missing or stale data:** quote staleness, crossed quotes, missing account summary, missing positions.
- **Slippage surprises:** realized fills materially worse than expected limit/slippage model.
- **Cancel races:** cancel rejected because order already filled, partial fill during kill/shutdown.
- **Clock anomalies:** host/TWS clock skew, skipped cron window, double-fire window.
- **Kill switch:** graceful shutdown, forceful cancellation fallback, remaining-order verification.
- **Audit issues:** corrupt JSONL, incomplete checkpoint sequence, sanitizer removing required evidence.

## Daily check-in template

Copy this section once per trading day during the soak.

### YYYY-MM-DD

- Equity/account snapshot: _TBD_
- Rebalance attempted? _yes/no_
- Orders submitted/filled/cancelled: _TBD_
- Reconcile result: _clean / mismatches listed above_
- Incidents observed: _none / links to incident log rows_
- Operator action taken: _none / details_
- Confidence note: _What did today prove or fail to prove?_

## Weekly summary template

### Week N

- Trading days completed this week: _TBD_
- Runs completed: _TBD_
- Incidents: _TBD_
- Manual interventions: _TBD_
- New bugs or patches: _TBD_
- Evidence links: _TBD_
- Decision: continue soak / restart from day 0 / stop and patch / release candidate ready

## What surprised us

Fill this only with evidence-backed surprises.

- _TBD_

Prompts for review:

- Did IBKR paper behave differently from mocks or failure-injection tests?
- Did any hardening path work but produce confusing operator output?
- Did any operational burden appear that the code technically handled but the workflow did not?
- Did any audit event fail to carry enough context to reconstruct the event later?

## What worked as designed

Record boring wins too. These are the strongest release notes.

- _TBD_

Examples to look for:

- Reconnect without duplicate orders.
- Idempotency rejected a duplicate cron window.
- Graceful shutdown completed with clean dangling-order verification.
- Forceful kill switch cancelled or verified open orders.
- Audit recovery identified an incomplete intent and selected the right recovery action.

## What broke or needed a patch

| Patch/commit | Trigger | Root cause | Why tests missed it | Restarted soak? |
| --- | --- | --- | --- | --- |
| _TBD_ | _TBD_ | _TBD_ | _TBD_ | yes/no |

Rule: any uncovered failure that changes production behavior should become a
v0.13.x patch and restart the soak from day 0, rather than being hidden inside a
v0.15 release candidate.

## What's still papered-over

This section is mandatory. Paper trading is not production.

Known gaps to revisit after the soak:

- **Paper exchange fills are not live liquidity.** Slippage and reject behavior may still differ from live trading.
- **Sanitized audit logs can remove context.** Verify the sanitizer keeps enough fields to reconstruct incidents.
- **Operator response is only partially tested.** Automated recovery can pass while human escalation remains unclear.
- **Long-running infrastructure remains local.** Cron, TWS/Gateway uptime, OS sleep, network changes, and disk pressure still need real deployment discipline.
- **Risk model is not strategy validation.** The soak validates plumbing and operations, not alpha or portfolio quality.

Add any newly discovered papered-over assumptions here before release.

## Release readiness conclusion

Do not fill this until the soak is complete.

- Recommendation: _release / extend soak / patch and restart / stop_
- Rationale: _TBD_
- Required follow-up beads/issues: _TBD_
- README battle-tested numbers confirmed? _yes/no_
- `examples/paper-live-ibkr/report.html` generated from sanitized audit excerpts? _yes/no_

## Links

- Runbook: `examples/paper-live-ibkr/README.md`
- Report generator: `examples/paper-live-ibkr/report.py`
- Sample sanitized audit excerpt: `examples/paper-live-ibkr/sample-audit.jsonl`
- Two-phase kill switch ops doc: `docs/ops/phase2-two-phase-kill.md`
- v0.15 kill-gate criteria: `docs/solutions/v0.15-kill-gate-criteria.md`
- v0.15 kill-gate evaluation: `docs/solutions/v0.15-kill-gate-evaluation.md`
