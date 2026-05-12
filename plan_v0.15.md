# nanobook v0.15 — "OCaml Oracle" — Plan (PREVIEW)

**Status:** PREVIEW — concrete spec to be written when v0.14 ships
**Target version:** v0.15.0
**Timeline:** 2–3 weeks (begins on a parallel branch during v0.14 soak)
**Baseline:** v0.14.0 (paper-live)

**Theme:** Introduce an OCaml limit-order-book reference oracle, used **CI-only** for differential testing against the Rust LOB. Users `pip install nanobook` and `cargo add nanobook` and never know OCaml exists.

---

## Honesty about the dual purpose

Adopted from the adversarial reviewer's "honest version": this release is *both* a technical bug-finding mechanism *and* a deliberate signaling choice toward the Jane Street / Swiss quant audience. Pretending it's purely one or the other invites the rationalization-spotting failure mode. Stated up front:

- **Technical purpose** (Codex framing): catches "wrong-but-consistent" business-logic bugs that fuzzing + mutation testing cannot — cases where Rust tests encode the same mistaken assumption as Rust code (Knight Capital bug class). An OCaml implementation written from spec alone, not from reading Rust source, breaks the coupling.
- **Signaling purpose** (adversary framing): OCaml in a Rust-quant repo signals familiarity with Jane Street's tooling lineage. Recruiters and peer engineers in the target audience will notice. The "invisible to user" constraint is what makes this honest rather than sloppy — users pay zero cost; the signal is for technical readers grepping the tree.

Both purposes are real. Stating both prevents the rationalization trap.

## Strict constraint: invisible to users

| Surface | Must contain OCaml? |
|---|---|
| `cargo add nanobook` consumers | No |
| `pip install nanobook` wheels | No |
| `docs.rs` / public Rust API | No |
| Default `cargo test` (no special features) | No (oracle is its own CI job) |
| `Cargo.toml` workspace members | No (oracle is a *sibling*, not a Cargo member) |
| CI pipeline | Yes (dedicated job) |
| Repo tree | Yes (`oracle-ocaml/` sibling dir) |
| Contributors running full test suite locally | Yes (opam required) |

Any drift from "invisible to users" is a release blocker.

## Architecture

```
nanobook/
├── Cargo.toml                 (workspace, unchanged)
├── src/                       (Rust LOB, unchanged)
├── oracle-ocaml/              ← NEW, separate dune project
│   ├── dune-project
│   ├── lib/
│   │   ├── price.ml
│   │   ├── order.ml
│   │   ├── book.ml            ← obvious-correctness LOB
│   │   └── matching.ml
│   ├── bin/
│   │   └── replay.ml          ← reads JSONL event log, emits JSONL trades
│   └── test/
└── .github/workflows/
    └── oracle.yml             ← runs Rust replay → OCaml replay → diff
```

The OCaml oracle is **deliberately slow and obvious**: sorted association lists for price levels, exhaustive pattern matching on every event variant, no clever data structures. Performance is irrelevant — correctness is the only goal.

**Library choice:** stock OCaml + Stdlib + Crowbar for property tests. Codex recommended this over Base/Core: lighter, ~800 LOC target, no Jane Street ecosystem dependency. The signal comes from *that there is an OCaml oracle at all*, not from which library it uses.

## Deliverables

1. **`oracle-ocaml/`** — ~600–1000 LOC OCaml, dune-built, fully self-contained.
2. **JSONL event-log schema** — formalized in `docs/event-log-schema.md` with explicit `schema_version` field. The Rust LOB emits this; the OCaml oracle consumes it. This becomes the contract between the two implementations.
3. **Golden corpus of LOB edge cases** (replaces the "must find ≥1 divergence" criterion that Codex flagged as theater): cancel-at-zero-quantity, multiple fills at the same price level, FOK no-match, FOK partial-cross, self-trade-prevention with each `StpPolicy` variant, order at min/max price, simultaneous-timestamp events. Both engines must produce byte-identical output on the corpus.
4. **`.github/workflows/oracle.yml`** — CI job: install OCaml (cached opam), build oracle, run golden corpus, replay v0.11 ITCH event log through both engines, diff trades. Fails the build on any divergence.
5. **`docs/solutions/oracle-design.md`** — design rationale, what the oracle is *for*, what bug classes it catches that v0.10 hardening doesn't, and the dual-purpose declaration above.
6. **CONTRIBUTING.md update** — section on how to run the oracle locally for contributors who want full coverage; explicit note that it's optional.

## Acceptance criteria

Revised after Codex review — the original "must find ≥1 divergence" criterion is removed:

- [ ] CI oracle job green on `main` for the **golden corpus** (mandatory; deterministic).
- [ ] CI oracle job green on `main` for the v0.11 ITCH 1-minute slice (mandatory; deterministic).
- [ ] If divergences are found during development, each is published as a triage example in `docs/solutions/oracle-divergences.md`. Zero divergences is acceptable — the corpus + ITCH replay still proves the engines agree on what we've tested.
- [ ] OCaml oracle has ≥90% line coverage via its own unit tests (independent of Rust↔OCaml diff).
- [ ] `cargo add nanobook` in a fresh project does NOT pull OCaml as a dependency (verified by a sanity-check CI job).
- [ ] `pip install nanobook` wheel build does NOT require OCaml (verified by a from-scratch Docker build).
- [ ] Default `cargo test` runs without OCaml installed.

## Risks

- **OCaml LSP/tooling friction.** Real cost. Mid-roadmap kill gate (decided in v0.13) covers this: >5 working days lost in week 1 → cut v0.15 from the roadmap.
- **LLM support is worse for OCaml than Rust.** Plan for more manual work; use Codex/Claude carefully, verify against `dune` build output.
- **Schema drift.** If the Rust LOB's event-log format changes, both engines must stay in sync. Mitigation: schema is a versioned doc, both sides parse the same JSON schema file, schema regression test in CI.
- **CI runtime cost.** OCaml install + build adds ~2–4 min per CI run. Cache aggressively.
- **Oracle bug masquerading as Rust bug (or vice versa).** When a divergence is found, both implementations must be inspected. Triage protocol documented up front in `oracle-design.md`.

## Version bumps

| Crate | v0.14.0 | v0.15.0 | Reason |
|---|---|---|---|
| `nanobook` | 0.14.0 | 0.15.0 | Possibly minor fixes from oracle-found divergences. Event log schema doc added with explicit `schema_version`. |
| `nanobook-broker` | 0.6.1 | 0.6.1 | Untouched. |
| `nanobook-risk` | 0.5.0 | 0.5.0 | Untouched. |
| `nanobook-rebalancer` | 0.7.1 | 0.7.1 | Untouched. |
| `nanobook-python` | 0.14.0 | 0.14.0 | Untouched. Oracle is CI-only and produces no Python-surface change. |

OCaml oracle is not versioned via crates.io — it's repo-internal tooling. Tracked via `oracle-ocaml/dune-project` only.

## Open questions

1. Whether to ALSO exercise the portfolio simulator via the oracle, or strictly LOB matching. Default: strictly LOB in v0.15; portfolio in v0.16 if scope allows.
2. Crowbar property tests in v0.15 or deferred to v0.16? Default: deferred — v0.15 is golden corpus + ITCH replay only.
3. How to handle the case where the OCaml oracle is *more* permissive than the Rust LOB (e.g., accepts an event Rust rejects). Acceptable: both engines should reject and produce the same error event in the output stream.

## Phasing (3 weeks, parallel branch during v0.14 soak)

| Week | Phase |
|---|---|
| 1 | Event-log schema formalization with `schema_version`; `oracle-ocaml/` scaffolding; price/order/side types; minimum-viable book |
| 2 | Matching engine; replay binary; golden corpus implementation |
| 3 | CI integration; cache tuning; design doc; release |
