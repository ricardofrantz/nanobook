# nanobook v0.11 — "Replay" — Plan (DRAFT, post-review)

**Date:** 2026-05-11
**Status:** DRAFT — revised after parallel external review (Codex + adversarial doc reviewer)
**Target version:** v0.11.0
**Timeline:** 2–3 weeks (smaller than v0.10, by design)
**Baseline:** v0.10.0 (shipped 2026-04-22, commit `4d914a7`)

**Theme:** First post-hardening release. Shift the repo from "library with claims" to "library with evidence." One reproducible public-data case study, no new public API, no breaking changes.

---

## Context

- v0.10.0 just shipped — pure hardening, zero new features (N-series correctness, S-series security, I-series supply-chain, D-series delivery).
- Repo positioning: "Rust execution layer for Python trading strategies" — IBKR/Binance adapters, deterministic LOB, portfolio simulator, risk engine, rebalancer CLI.
- README quotes ~120–200 ns/order and ~6M ops/sec from synthetic microbenchmarks. No public artifact today demonstrates this on real exchange data.
- Python bindings via PyO3 remain in tree (the `python/` deletion was accidental staging — restored).
- Constraint: future OCaml work must be invisible to users (CI-only differential oracle, deferred to v0.15).

## Review-driven revisions (vs pre-review draft)

- **BBO acceptance criterion dropped.** NASDAQ's published BBO/QBBO file is a separately licensed product (Nasdaq Basic), not freely available alongside ITCH samples. Replaced with **invariant self-checks** (no crossed book, monotonic timestamps, cancel reduces resting quantity to zero, conservation of total volume).
- **Both perf numbers kept, not replaced.** Measured replay numbers (p50/p95/p99 on real ITCH) move top-of-fold. Synthetic criterion microbench stays as a "kernel microbenchmarks" subsection with the existing methodology footnote. Adding data > replacing data.
- **`REPRODUCIBILITY.md` added as a v0.11 deliverable** — Codex flagged this as half a day of work that multiplies credibility on every claim.

## Goal

Ship one reproducible, public-data case study: a NASDAQ TotalView-ITCH 5.0 trading day replayed through nanobook's LOB, producing a notebook/report + writeup that anyone cloning the repo can reproduce in <10 minutes on a laptop.

The artifact answers a question nanobook's README currently can't: *"On real exchange data, with real cancel-heavy order mix, what does this thing actually do?"*

## Non-goals (v0.11)

- No new public Rust or Python API.
- No breaking changes — minor version bump only.
- No live broker work (deferred to v0.14).
- No ops hardening / failure injection (v0.13).
- No backtest framework changes (v0.12).
- No OCaml, no FFI, no new workspace members (v0.15+).

## Deliverables

1. **`examples/itch-replay/`** — self-contained directory:
   - `README.md` — what it shows, how to reproduce, expected output, methodology
   - `download.sh` — fetches a public NASDAQ TotalView-ITCH 5.0 sample from `emi.nasdaq.com`, checksummed (md5sum confirmed available alongside the `.NASDAQ_ITCH50.gz` files)
   - `replay.rs` (cargo example or `src/bin/`) — streams ITCH events into the LOB, emits JSONL event log + summary stats + LOB invariant self-check log
   - `report.py` (preferred over Jupyter — diffable, no notebook rot) — loads event log via PyO3 bindings, emits `report.html` with: book-reconstruction snapshots, top-of-book spread distribution, message-rate timeline, p50/p95/p99 latency histogram (parse / book-update / strategy-to-order breakdowns)
   - `expected/` — golden output for the 1-minute CI slice
2. **`REPRODUCIBILITY.md`** at repo root — exact dataset URL, md5sum, exact command sequence, hardware model + clock speed, OS version, Rust toolchain version, Python version, `uv.lock` hash, CI slice parameters, explanation of why raw licensed data is not vendored.
3. **`docs/solutions/itch-replay-learnings.md`** — surprises, residual issues, follow-up candidates for v0.12.
4. **README perf section rewrite** — measured numbers from the replay top-of-fold; synthetic criterion microbench retained in a "Kernel microbenchmarks" subsection with the existing methodology footnote. Both numbers, clearly labeled.
5. **CI `examples-smoke` job** — downloads a 1-minute slice, runs the full replay end-to-end, diffs against `expected/`. Full-day replay stays out of CI (size).

## Acceptance criteria

Each must be demonstrable:

- [ ] Fresh clone → `bash examples/itch-replay/download.sh && cargo run --release --example itch-replay && uv run python examples/itch-replay/report.py` completes on a 16 GB laptop in <10 min wall time.
- [ ] **LOB invariant self-checks pass for the full replay window**: no crossed book at any tick, all timestamps monotonic per-symbol, every cancel reduces resting quantity (or removes the order), aggregate volume conserved across event boundaries. (Replaces the BBO-against-published-file criterion.)
- [ ] Measured p50/p95/p99 latency histogram published in README top-of-fold. Synthetic criterion microbench retained in subsection. Methodology footnote points to `examples/itch-replay/README.md` + `REPRODUCIBILITY.md`.
- [ ] `cargo test --workspace --all-features` and `cargo clippy --workspace --all-targets --all-features -- -D warnings` green.
- [ ] CI `examples-smoke` job green on every PR.
- [ ] `REPRODUCIBILITY.md` committed, contents verified by a contributor (or by re-running on a different machine).
- [ ] `docs/solutions/itch-replay-learnings.md` committed and indexed.
- [ ] `CHANGELOG.md` updated; version bumps applied (see §Version bumps).

## Risks

- **Data licensing.** NASDAQ TotalView-ITCH samples confirmed freely downloadable; redistribution rules vary. Mitigation: `download.sh` fetches at user time; no ITCH bytes are committed. **QBBO/published-BBO is licensed and out of scope** — invariant self-checks replace external comparison.
- **CI runtime + size.** Full ITCH day is 3.5–6 GB compressed. CI must use a deterministic 1-minute slice. Slice generator must produce byte-identical output across runs.
- **`report.py` reproducibility.** Pin Python deps in `examples/itch-replay/requirements.txt`; run through `uv`. Avoid Jupyter (rots).
- **Measured perf may be worse than headline.** Feature, not risk. With both numbers in the README, the synthetic stays as the "kernel ceiling" and the measured number is the "real-world end-to-end including parse + log + match." Two honest numbers measuring two different things.
- **Existing ITCH parser may have gaps.** v0.10 added fuzzing on ITCH (I2) but real NASDAQ feeds include edge cases the fuzzer may not cover. If gaps surface, fix in v0.11 (counts as "demo-driven hardening").

## Version bumps

| Crate | v0.10.0 | v0.11.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.10.0 | 0.11.0 | New `examples/itch-replay/`. Possible internal ITCH parser fixes if replay exposes gaps. No public API change. |
| `nanobook-broker` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.6.0 | 0.6.0 | Untouched. |
| `nanobook-python` | 0.10.0 | 0.11.0 | Re-export only; demo `report.py` uses existing bindings. |

Only the top-level workspace crate(s) bump. No breaking changes.

## Workflow

Per v0.10 conventions (`plan_v0.10.md` §0):

- Small commits per logical unit. Conventional Commits.
- `git commit -F <file>` for any body with backticks/newlines.
- Named staging only. Never `git add -A` or `git add .`.
- Per-commit verification on touched scope; full workspace before push.
- Phase gates with explicit verification check per phase (see §Phasing).

## Phasing (3 weeks)

| Week | Phase | Verification |
|---|---|---|
| 1 | E1 Restore `python/` staging; pick ITCH source; license verification; `download.sh` + checksum; CI 1-minute slicer; draft `REPRODUCIBILITY.md` skeleton | `bash download.sh && sha256sum -c expected/sample.sha256` passes |
| 1 | E2 `cargo run --example itch-replay` consumes the sample, emits JSONL event log + summary + invariant-check log | Event count + trade count match deterministic expected values; invariant log empty for the slice |
| 2 | E3 `report.py` produces `report.html` with measured p50/p95/p99 (parse / book-update / strategy-to-order); spread + message-rate plots | All histograms render; numbers are reproducible across two laptop runs |
| 2 | E4 README perf rewrite: measured top-of-fold, synthetic in "Kernel microbenchmarks" subsection, methodology footnote points to repro doc | `grep -q "Measured on" README.md` and synthetic table still present with explicit "kernel microbench" label |
| 3 | E5 `REPRODUCIBILITY.md` finalized with all metadata; `docs/solutions/itch-replay-learnings.md` written; `CHANGELOG.md` and version bumps committed | Pre-release checks green; tag dry-run succeeds |
| 3 | E6 Release: tag, publish, announce | `cargo publish --dry-run` clean; tag pushed |

## Out of scope (v0.11) → revised roadmap

| Version | Theme | Scope budget | User-visible? |
|---|---|---|---|
| **v0.11** | **ITCH replay case study + `REPRODUCIBILITY.md`** (this doc) | 2–3 weeks | Yes |
| v0.12 | Backtest case study + analytics depth + **competitive positioning table** in README | 2–3 weeks | Yes |
| v0.13 | **Ops hardening — failure injection, reconnect, kill switch, idempotency, warm restart** (was the build phase of "Paper Live"; soak split out) | 2–3 weeks | Yes (broker + rebalancer changes) |
| v0.14 | **Paper-Live Soak** — uses v0.13's hardened plumbing, 2–4 week calendar soak. Runs *in parallel* with v0.15+ development so calendar time isn't on the critical path | 2–4w wallclock | Yes |
| v0.15 | OCaml differential oracle, **CI-only, invisible to users**. Replaces v0.11's invariant-only validation with cross-implementation diff. Dual-purpose declared honestly (technical + signaling) | 2–3 weeks | **No** |
| v0.16 | Oracle hardening + property tests (Crowbar) + cross-version replay gating | 1–2 weeks | **No** |
| v0.17 | **"Why nanobook stays 0.x"** — `docs/staying-0x.md` + public-surface audit + MSRV pin + semver policy. **NO 1.0 tag planned.** | 1–2 weeks | Yes (semver implications) |

Total arc: ~14–18 weeks across seven releases. Each ships independently; the repo tells a coherent story after any of them.

**Mid-roadmap kill gate after v0.14 (paper-live soak):** Pre-defined cut criteria for v0.15–v0.17, decided *before* v0.13 starts:
- Hours/week on nanobook over the prior 8 weeks: kill if <5
- Unplanned ops fixes during v0.13: kill threshold ≥3
- OCaml setup blockers in v0.15 week 1: kill if >5 working days lost
- Burnout self-report: hard stop

## Open questions (carried forward)

1. NASDAQ TotalView-ITCH 5.0 confirmed as source. Specific date TBD (2019-01-30 or later — pick a date with deep enough volume to exercise cancel-heavy flow).
2. Slice length for CI: 1 minute (current) or 30 seconds? Driven by CI wall-time budget.
3. Hardware row for `REPRODUCIBILITY.md`: laptop only, or include a reference server result too?

## Success definition

v0.11 is successful when an outsider clones the repo, follows `REPRODUCIBILITY.md`, runs three commands, gets a rendered HTML report showing nanobook reconstructing a real exchange day with honest latency numbers — and the README points there for measured perf, alongside the retained synthetic microbenchmark.
