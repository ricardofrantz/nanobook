# nanobook documentation

This directory contains public project documentation for nanobook. It focuses on versioning policy, public API shape, deterministic event replay, operational safety, and implementation learnings that are useful to contributors and users.

## Project policy

- [Versioning policy](../SEMVER.md) — how nanobook treats SemVer while it remains pre-1.0.
- [Why nanobook stays 0.x](staying-0x.md) — rationale for delaying a 1.0 stability promise.

## Domain and API reference

- [Ubiquitous language](../UBIQUITOUS_LANGUAGE.md) — glossary for order lifecycle, portfolio, risk, broker, and rebalancer terms.
- [Event log schema](event-log-schema.md) — JSONL schema for deterministic order-book replay and oracle compatibility.
- [Public API surface audit](api-surface-audit.md) — inventory of exported Rust/Python surface used to reason about stability.
- [Public API baselines](public-api/) — generated `cargo-public-api` snapshots for workspace crates.

## Rebalancer operations

- [Rebalancer operations hardening](operations/rebalancer-ops-hardening.md) — failure modes and hardening patterns.
- [Write-ahead audit logging](operations/write-ahead-audit-logging.md) — intent/result checkpoints for broker operations.
- [Warm restart](operations/warm-restart.md) — audit-log recovery after a crash or broker restart.
- [Graceful shutdown](operations/graceful-shutdown.md) — safe SIGTERM behavior during a rebalance run.
- [Kill switch](operations/kill-switch.md) — emergency stop and optional forceful broker cancellation workflow.

## Learnings and design notes

- [OCaml oracle implementation summary](ocaml-oracle-v0.15-summary.md) — reference oracle architecture and implementation notes.
- [Oracle design](solutions/oracle-design.md) — why the OCaml oracle exists and how to triage divergences.
- [ITCH replay learnings](solutions/itch-replay-learnings.md) — performance methodology and replay harness lessons.
- [Portfolio simulator parity learnings](solutions/portfolio-sim-parity-learnings.md) — comparison against vectorbt and known differences.

## Audits

- [Unsafe code audit summary](audits/unsafe-code-audit-2026-05-15.md) — summary of unsafe-code review findings.
