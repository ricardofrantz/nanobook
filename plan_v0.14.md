# nanobook v0.14 — "Paper-Live Soak" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.13 ships
**Target version:** v0.14.0
**Timeline:** 2 weeks build prep + **2–4 weeks paper-trading soak (runs in parallel with v0.15 development)**
**Baseline:** v0.13.0 (ops-hardened broker + rebalancer)

**Theme:** Run v0.12's strategy through nanobook's hardened plumbing against an IBKR paper account for a multi-week soak, with full audit-log publication. The artifact moves nanobook from "library that works on historical data" to "library that ran live against a real (paper) exchange and survived."

---

## Critical scheduling change vs original plan

The original "Paper Live" plan put a 2–4 week calendar soak on the critical path of the roadmap. Both reviewers flagged this as the biggest collapse risk. **Revised:** the soak runs *in parallel* with v0.15 (OCaml oracle) development. v0.15 work proceeds on a separate branch; v0.14's release tag is cut when the soak completes and the writeup is done, regardless of v0.15's progress.

Practically:
- Week 1: pre-soak rehearsal against IBKR paper (1-week dry run, verifying v0.13's failure-injection hardening holds).
- Week 2+: soak begins. Author starts v0.15 OCaml oracle work on a parallel branch.
- Week 4–6 (calendar): soak completes; v0.14 writeup; v0.14 tagged.
- v0.15 may already be partially done by then.

## Goal

Publish a real (paper) trading run, end-to-end through nanobook: weekly strategy decisions → rebalancer dry-run → confirmation → execution → audit log → equity curve. Reader can inspect every decision, every order, every reconcile.

## Caveats stated up-front

IBKR paper trading uses simulated fills based on top-of-book quotes without actual market depth, delayed market data unless paid-for, and a subset of order types. "Didn't blow up" on paper is only meaningful because v0.13 already proved the plumbing handles failure modes the paper environment will exercise rarely. The writeup must state this explicitly.

## Non-goals

- No real-money. Paper account only.
- No new strategy work — v0.12's momentum strategy is the source of weights.
- No new failure-injection (v0.13 already covers this).
- No OCaml work in this release (parallel branch, lands in v0.15).
- No new venues.

## Deliverables

1. **`examples/paper-live-ibkr/`**:
   - `README.md` — what was run, when, on what universe, with what risk limits
   - `runner.sh` — cron-friendly script using `rebalancer --cron-mode` (v0.13)
   - `risk-config.toml` — published risk limits used during the soak
   - `audit/` — committed audit-log excerpts for the soak window, sanitized via a published scrubber
   - `report.py` → `report.html` — daily equity curve, position evolution, reconcile mismatches (if any), realized vs expected slippage, list of every operational incident during the soak
2. **`scripts/sanitize-audit.py`** — publishable scrubber that removes account IDs, client IDs, IBKR order IDs while preserving the audit-log invariants (sequence numbers, timestamps, order math). Idempotent.
3. **`docs/solutions/paper-soak-learnings.md`** — what broke, what didn't, what's still papered-over. Honest writeup of every operational incident.
4. **README "Battle-tested" section** — link + headline summary of the soak (calendar duration, number of rebalances, number of orders, number of reconciles, number of incidents).

## Acceptance criteria

- [ ] At least 14 trading days of paper soak completed without crash or manual intervention beyond scheduled rebalances.
- [ ] All rebalances logged. Audit log replay (via v0.13's warm-restart protocol) reproduces the published equity curve exactly.
- [ ] Any divergence between expected and realized fills traced and documented.
- [ ] **No real-money credentials, account numbers, or PII leak into committed audit excerpts.** Sanitization verified by a contributor or by `scripts/sanitize-audit.py --check`.
- [ ] Honest writeup of every operational incident — reconnects, rejects, missing market data, slippage surprises — published in the soak learnings doc.

## Risks

- **IBKR paper API flakiness.** Mitigated by v0.13's hardening. Any failure mode v0.13 didn't cover is a v0.13 bug (loops back to a v0.13.1 patch).
- **Sanitization leak.** Manual review gate required before any `audit/` excerpt is committed. Two-person review if possible (or model-assisted second look).
- **Strategy may underperform on paper.** Doesn't matter for the demo — the artifact is the *plumbing*, not the alpha. Document this explicitly.
- **Time-zone / market-calendar bugs.** Likely to surface in production for the first time. If not caught by v0.13, counts as a v0.14.1 patch.
- **Soak time eats real calendar time.** Mitigated by parallel v0.15 work.

## Version bumps

| Crate | v0.13.0 | v0.14.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.13.0 | 0.14.0 | Likely calendar / TZ fixes that surface during the live paper soak. |
| `nanobook-broker` | 0.6.0 | 0.6.1 | Patch: non-breaking soak-found fixes (reconnect timing, callback dedup edge cases). |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.7.0 | 0.7.1 | Patch: non-breaking TZ/calendar fixes that surface during the soak. |
| `nanobook-python` | 0.13.0 | 0.14.0 | Re-exports + `scripts/sanitize-audit.py` integration. |

If the soak surfaces a **breaking** broker or rebalancer bug, the response is a v0.13.1 patch + soak restart, **not** a v0.14 minor bump rolling in breaking changes — patch line stays patch line.

## Open questions

1. Public soak runner host: GitHub Actions self-hosted runner on a small Mac mini? A cheap VPS? Author's laptop with a cron job?
2. Universe size: keep v0.12's S&P 100 monthly, or shrink to 10 names weekly for clearer audit-log storytelling?
3. Disclosure: publish the soak as it happens (live blog) or after completion (post-mortem)? Default: post-mortem (less pressure).
4. If soak surfaces a v0.13 bug, do we patch v0.13 first and restart soak from day 0, or proceed and document?

## Phasing

| Phase | Duration | Content |
|---|---|---|
| Pre-soak rehearsal | 1 week | Live IBKR paper dry-run validating v0.13 hardening holds in the real paper environment |
| Soak | 2–4 calendar weeks | Live paper run; daily check-ins; weekly summaries |
| Parallel | (during soak) | v0.15 OCaml oracle development begins on a parallel branch |
| Writeup | 1 week | Report polish, learnings doc, release |

## Mid-roadmap kill gate (after v0.14)

Per pre-agreed criteria written into `plan_v0.13.md`:
- Hours/week on nanobook over the prior 8 weeks: kill v0.15–v0.17 if <5
- Unplanned ops fixes during v0.13: kill threshold ≥3
- OCaml setup blockers in v0.15 week 1 (parallel branch): kill if >5 working days lost
- Burnout self-report: hard stop

The kill decision is binary: continue to v0.15, or cut the roadmap to "v0.14 is the last release before a long pause." Either is acceptable. The plan must support stopping here cleanly — v0.14's writeup is self-contained.
