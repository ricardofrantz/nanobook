# nanobook v0.17 — "Why nanobook stays 0.x" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.16 ships
**Target version:** v0.17.0
**Timeline:** 1–2 weeks
**Baseline:** v0.16.0 (oracle hardened + property tests)

**Theme:** **Inverted from the original v0.16 "API Freeze Candidate" plan.** Rather than planting a 1.0 flag the project cannot maintain, v0.17 explicitly argues why nanobook stays pre-1.0. This is the stronger position for a single-maintainer project — it demonstrates that the author understands what 1.0 actually commits to.

---

## Why this exists (adversarial-reviewer framing, adopted)

A 1.0 tag commits to:

- Semver-major stability indefinitely.
- A user base the project doesn't yet have.
- Implicit promise of CVE-response speed, MSRV updates, issue triage.

For a solo-maintained project using LLM workers, this promise will likely be broken within 18 months. A stale 1.0 is *worse* than an actively-developed 0.x — it signals "abandoned production-ready project" to anyone discovering the repo later.

**Inversion:** publish a written argument for why nanobook stays 0.x. This is rare among open-source quant projects (most tag 1.0 prematurely) and demonstrates engineering maturity. It costs nothing and doesn't preclude a real 1.0 in some future year if the situation changes.

## Goal

Ship `docs/staying-0x.md` plus the public-surface audit and policy docs that would have been needed *anyway* for a 1.0 candidate — but framed as "here's what we know and don't promise" rather than "here's what we lock down."

## Deliverables

1. **`docs/staying-0x.md`** — the central artifact. Explicit argument:
   - Nanobook is single-maintainer infra; 1.0 promises stability it cannot underwrite alone.
   - Pre-1.0 status lets us evolve the LOB schema, audit format, broker trait as we learn from real deployments (v0.11 ITCH, v0.14 soak, v0.15 oracle).
   - Users needing 1.0-style stability should fork at a tag and own the API.
   - Conditions under which we *would* revisit 1.0: ≥3 active maintainers, ≥6 months of stable schema, demonstrated user base.
2. **Public-surface audit** — every `pub` item across all crates inventoried with: stability status, deprecation candidates, doc completeness. Output: `docs/api-surface-audit.md`. Same content as a 1.0 audit; different framing.
3. **Deprecation cleanup** — remove any `#[deprecated]` items that have been deprecated since v0.9 or earlier.
4. **`SEMVER.md`** — written semver policy, but explicitly stating "0.x means minor versions may break." Documents:
   - What counts as a public API for nanobook
   - What counts as breaking
   - What is explicitly NOT covered (event log byte format, internal modules, `#[doc(hidden)]` items)
5. **MSRV statement** — pin a minimum supported Rust version, document it, gate CI.
6. **Doc audit** — every public item has a rustdoc example; missing ones added.

## Acceptance criteria

- [ ] `cargo doc --workspace --no-deps --all-features` builds with zero warnings.
- [ ] `cargo public-api` (or equivalent) baseline locked; CI fails on unexpected API changes — but baseline is documentation, not contract.
- [ ] MSRV is pinned and CI matrix includes the MSRV row.
- [ ] All case-study demos (v0.11, v0.12) and the oracle gate (v0.15, v0.16) still green on this release.
- [ ] `docs/staying-0x.md` ships with explicit decision and the conditions under which 1.0 could be reconsidered later.

## Risks

- **Audit surfaces unwanted breaking changes.** Better now than after 1.0. If found, batch and ship as v0.17 (still pre-1.0, breaking is allowed).
- **MSRV pin too aggressive or too conservative.** Pick based on what the demos actually need; document the choice.
- **"Staying 0.x" reads as cop-out to some readers.** Mitigation: the doc must argue the position positively (what 0.x *enables*) rather than defensively (what we *can't* promise).

## Version bumps

| Crate | v0.16.0 | v0.17.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.16.0 | 0.17.0 | Public-surface audit + deprecation cleanup. Breaking changes allowed (pre-1.0). |
| `nanobook-broker` | 0.6.1 | 0.7.0 | Audit-driven removals of any items deprecated since v0.9 or earlier. **Breaking** (pre-1.0 minor). |
| `nanobook-risk` | 0.5.0 | 0.6.0 | First substantive change since v0.10. Audit-driven cleanup. **Breaking** (pre-1.0 minor). |
| `nanobook-rebalancer` | 0.7.1 | 0.8.0 | Audit-driven cleanup. **Breaking** (pre-1.0 minor). |
| `nanobook-python` | 0.14.0 | 0.15.0 | Re-exports follow upstream breaks. Python public surface gets its own audit row in `api-surface-audit.md`. |

**Per-crate semver, not a unified workspace version.** By v0.17 the workspace looks like:

| Crate | Final | Trajectory |
|---|---|---|
| `nanobook` | 0.17.0 | 0.10 → 0.11 → 0.12 → 0.13 → 0.14 → 0.15 → 0.16 → 0.17 (bumps every release) |
| `nanobook-python` | 0.15.0 | 0.10 → 0.11 → 0.12 → 0.13 → 0.14 → 0.14 → 0.14 → 0.15 (stable through v0.15/v0.16) |
| `nanobook-broker` | 0.7.0 | 0.5 → 0.5 → 0.5 → 0.6 → 0.6.1 → 0.6.1 → 0.6.1 → 0.7 |
| `nanobook-risk` | 0.6.0 | 0.5 → 0.5 → 0.5 → 0.5 → 0.5 → 0.5 → 0.5 → 0.6 (one change in seven releases) |
| `nanobook-rebalancer` | 0.8.0 | 0.6 → 0.6 → 0.6 → 0.7 → 0.7.1 → 0.7.1 → 0.7.1 → 0.8 |

The version sprawl is intentional. Each crate's number reflects its own change history. Synchronizing all five crates to 0.17.0 would be marketing, not semver — `nanobook-risk` has had effectively one change in seven releases; calling it 0.17 would dishonestly claim seven releases of churn.

## Open questions

1. Does nanobook have a `python` package on PyPI that needs the same audit? Yes — Python public surface needs its own pass.
2. `cargo public-api` vs `cargo-semver-checks` vs manual diffs?
3. Do we publish a "what semver covers" tutorial alongside `SEMVER.md`?

## Phasing (2 weeks)

| Week | Phase |
|---|---|
| 1 | API surface audit; deprecation cleanup; `SEMVER.md` draft; `staying-0x.md` v1 |
| 2 | Doc gaps closed; MSRV pinning; `staying-0x.md` finalized; release |

## What this commits to (and doesn't)

- **Commits to:** an honest framing of project maturity, a working `SEMVER.md`, a clean docs surface, an MSRV pin.
- **Does NOT commit to:** a future 1.0 timeline, semver-major stability, response-time SLAs, or maintenance promises beyond best-effort.

This is the stronger position. A future 1.0 — if it happens — would be earned by satisfying the conditions stated in `staying-0x.md`, not announced as a marketing milestone.
