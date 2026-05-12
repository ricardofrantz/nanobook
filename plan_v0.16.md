# nanobook v0.16 — "Oracle Hardening" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.15 ships
**Target version:** v0.16.0
**Timeline:** 1–2 weeks
**Baseline:** v0.15.0 (OCaml oracle introduced)

**Theme:** Take the oracle from "exists" to "load-bearing." Property testing on the OCaml side, cross-version replay gating, triage tooling, and schema versioning. Still invisible to users.

---

## Goal

Make the oracle catch *novel* bugs, not just regressions. Move from "diff Rust output against OCaml on a fixed input" to "generate adversarial inputs, replay both, prove no divergence exists across the input space."

## Deliverables

1. **OCaml property tests via Crowbar** — generate random valid event streams, replay through both engines, assert trade-output equality. Adds genuine fuzz coverage to the OCaml side, independent of the Rust fuzz harness.
2. **Cross-version replay gating** — every PR replays v0.11's full ITCH 1-minute slice AND v0.12's backtest event log AND v0.14's paper-soak audit excerpt through both engines. The oracle becomes a regression gate for *all* prior demos.
3. **Oracle-failure triage tool** — `oracle-ocaml/bin/triage.ml` that, given a divergent event stream, bisects to find the minimal failing input. Speeds up debugging when a new divergence appears.
4. **Schema versioning** — formal `schema_version` field (added in v0.15), with explicit compat policy in `docs/event-log-schema.md`. Schema bumps are breaking and must update both engines.
5. **`docs/solutions/oracle-property-tests.md`** — what property tests caught vs what hand-written tests caught.

## Acceptance criteria

- [ ] Property test runs in CI on every PR with a fixed seed budget (e.g., 10k random event streams).
- [ ] Triage tool reduces any divergent input to ≤20 events.
- [ ] Schema version bump regression test green.
- [ ] Cross-version gate green: all prior demos' event logs pass the oracle on `main`.
- [ ] Still no user-visible OCaml. `pip install nanobook` wheel check still passes.

## Risks

- **Property tests find legitimate Rust bugs.** Good problem, but releases get blocked until fixed. Triage protocol from v0.15's `oracle-design.md` covers this.
- **OCaml-side flakiness from RNG.** Use deterministic seeds in CI; record + replay failing seeds.
- **CI time creep.** Property tests + multi-log replay add minutes. Parallelize where possible; cap seed budget.

## Version bumps

| Crate | v0.15.0 | v0.16.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.15.0 | 0.16.0 | Likely minor fixes from property-test-found divergences. |
| `nanobook-broker` | 0.6.1 | 0.6.1 | Untouched. |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.7.1 | 0.7.1 | Untouched. |
| `nanobook-python` | 0.14.0 | 0.14.0 | Untouched. |

## Open questions

1. Crowbar (AFL-style, fast) confirmed for v0.15+; revisit if shrinking matters more than speed.
2. Do property tests also cover the portfolio simulator, or strictly LOB? Default: LOB only.
3. Should the triage tool become a separate `cargo install nanobook-triage` user-facing tool, or stay CI-only? **Default: CI-only**, per invisibility constraint.

## Phasing (2 weeks)

| Week | Phase |
|---|---|
| 1 | Crowbar harness; first 1k-seed run; debug initial divergences; triage tool MVP |
| 2 | Cross-version replay gating; schema versioning; learnings doc; release |
