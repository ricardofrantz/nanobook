# nanobook v0.13 — "Ops Hardening" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.12 ships
**Target version:** v0.13.0
**Timeline:** 2–3 weeks
**Baseline:** v0.12.0 (backtest case study + positioning)

**Theme:** Production-readiness for the broker + rebalancer path. Failure injection, reconnect, kill switch, idempotency, warm-restart. This release was originally bundled with the paper-live soak — split out so the soak (v0.14) runs against *already-hardened* plumbing, not as a discovery exercise.

---

## Why split from paper-live

Both external reviewers flagged the original v0.13 ("paper-live soak") as the weakest release. Two independent reasons:

- **Adversarial reviewer:** calendar-time soaks shouldn't be on the critical path of a software roadmap.
- **Codex reviewer:** running a paper soak *before* failure injection produces weak evidence — "didn't blow up" on a calm IBKR paper account is not the same as "would survive a reconnect mid-position." Failure injection has to come first.

v0.13 is the failure-injection release. v0.14 is the soak. Soak now runs in parallel with v0.15+ development.

## Goal

Make the IBKR broker adapter + rebalancer survive the realistic failure modes that will hit any live deployment. Catch the bugs in CI / synthetic environments, not during the soak.

## Failure modes to cover

Drawn from Codex's review + standard live-trading practice (Hummingbot, NautilusTrader playbooks):

1. **Duplicate order-status callbacks** from IBKR (TWS occasionally re-sends).
2. **Cancel reject** (order already filled; race between cancel request and fill).
3. **Partial fill followed by disconnect.** Reconnect must observe the partial state, not double-submit.
4. **Stale market data** (snapshot held while quotes move; rebalancer must detect + abort).
5. **Clock skew** between strategy host and exchange (NTP drift, VM clock jumps).
6. **TWS restart mid-position.** Reconnect drill: process survives, state reconciles from audit log + IBKR's open-positions query.
7. **Cron double-fire** (idempotency proof: rebalancer with `--cron-mode` can run twice in <1s and the second run is a no-op).
8. **Kill switch.** Operator can halt the runner without leaving dangling orders. Verifiable by audit-log inspection.
9. **Process crash mid-rebalance.** Restart reads audit log, determines where it was, completes or rolls back cleanly.

## Deliverables

1. **`broker/tests/failure_injection/`** — synthetic harness that mocks IBKR's API at the wire level and injects each of the failure modes above. Asserts the broker adapter responds correctly (idempotent retry, abort, reconcile).
2. **`rebalancer/tests/ops/`** — integration tests for cron double-fire, kill switch, warm-restart-from-audit-log.
3. **`rebalancer --kill` subcommand** — sends SIGTERM-equivalent to a running runner via lockfile/PID, verifies no dangling orders by querying audit log.
4. **`rebalancer --cron-mode` flag** — idempotent rebalance: writes a sequence number to audit log, refuses to run if a sequence with the same window is already complete.
5. **Warm-restart documentation** — `docs/ops/warm-restart.md` covering the audit-log → state reconstruction protocol with worked examples.
6. **`docs/solutions/ops-hardening-learnings.md`** — what each failure mode taught us; which were already handled by v0.10 hardening and which surfaced new bugs.
7. **CI extension** — failure-injection suite runs on every PR.

## Acceptance criteria

- [ ] All nine failure modes covered by automated tests in CI.
- [ ] `rebalancer --cron-mode` proven idempotent: 1000 back-to-back invocations produce exactly one set of orders.
- [ ] Warm-restart drill: kill the runner mid-rebalance via SIGKILL, restart, audit log replay produces the correct final position state. Verified end-to-end in CI.
- [ ] Kill switch test: `rebalancer --kill` halts the runner and leaves no working orders on the (mocked) exchange.
- [ ] Reconnect drill scripted: TWS restart simulated mid-position, runner reconnects and reconciles within 30s.

## Risks

- **Mocking IBKR's wire protocol faithfully.** TWS API has quirks (callback ordering, sequence numbers, partial-fill semantics) that won't show up in a hand-rolled mock. Mitigation: validate the mock against the IBKR paper account by running both side-by-side for a session — any divergence is a mock bug.
- **Scope creep.** "Production-ready" is a moving target. Lock the nine failure modes above; defer anything else to v0.14+ (or its own release).
- **Tests too coupled to IBKR.** Binance adapter should get at least 2 of these (idempotency + reconnect) — defer the rest to v0.16 if scope tight.

## Version bumps

| Crate | v0.12.0 | v0.13.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.12.0 | 0.13.0 | Audit-log replay helpers, calendar/TZ fixes likely surface during failure injection. |
| `nanobook-broker` | 0.5.0 | 0.6.0 | Idempotency, reconnect, audit-log integration likely produce minor signature changes. **Breaking** (pre-1.0 minor bump). |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.6.0 | 0.7.0 | New `--cron-mode` and `--kill` subcommands; warm-restart protocol. **Breaking** (pre-1.0 minor bump). |
| `nanobook-python` | 0.12.0 | 0.13.0 | Re-exports. |

## Open questions

1. Failure-injection harness language: pure Rust integration test, or a Python-driven scenario runner that exercises the binary?
2. Binance coverage: 2 modes (Codex suggested) or full parity with IBKR?
3. Audit-log replay: pure-replay (read-only) or also write back a "recovery completed" marker?

## Phasing (3 weeks)

| Week | Phase |
|---|---|
| 1 | Failure-injection harness scaffold; failure modes 1–3 (callbacks, cancel reject, partial fill + disconnect) |
| 2 | Failure modes 4–6 (stale data, clock skew, TWS restart); reconnect drill |
| 3 | Idempotency + kill switch + warm restart; docs; release |

## Out of scope → v0.14

- Actual paper-live soak (v0.14)
- Binance failure-injection beyond the 2 minimal modes (v0.16 if needed)
