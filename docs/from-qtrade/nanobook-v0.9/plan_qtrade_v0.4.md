# plan_qtrade_v0.4

## Mission
Ship `qtrade v0.4.0` as a production release with:
1. stable data platform (Postgres + Parquet + DuckDB),
2. strict ingestion/versioning contracts,
3. operational readiness (scheduling, observability, rollback),
4. nanobook v0.9 integration hooks (built in parallel, not blocked here).

## Program Constraints
1. Primary runtime environment: macOS (dev/prod baseline for now).
2. `nanobook v0.9` implementation happens in a separate session/repo.
3. qtrade must remain correct with nanobook fully disabled.
4. qtrade must also support partial nanobook capability sets.

## Target End State (v0.4.0)
1. Historical market data is managed in Parquet lake, not Git.
2. Postgres is the metadata control plane for lineage, quality, runs, and leases.
3. DuckDB is the analytical SQL engine over lake data for research/backtests.
4. Ingestion is idempotent, validated, and manifest-driven.
5. nanobook feature-gated integration is ready for v0.9 APIs.
6. Production runbooks, alerts, and release gates are in place.

## Architecture Decision
1. `Postgres`: operational metadata, job coordination, lineage, quality outcomes.
2. `Parquet Lake`: authoritative historical datasets (bronze/silver/gold tiers).
3. `DuckDB`: fast OLAP queries over Parquet for strategy research/backtest workloads.
4. `Polars`: pipeline transform engine for pull/prep/calc modules.
5. `nanobook`: accelerated quant compute path when available.

## Workstreams

## W1: Data Platform Foundation
### Scope
1. Adopt metadata schema: `deploy/postgres/schema_v1.sql`.
2. Use manifest contract: `deploy/contracts/ingestion_manifest.schema.json`.
3. Define lake layout/partitioning standard and retention policy.

### Deliverables
1. Postgres initialized with schema v1 in each env.
2. Dataset registry seeded (`datasets`, `symbols`).
3. Data zones enforced: `bronze`, `silver`, `gold`.

### Acceptance
1. Every ingestion run writes a Postgres run row + version row.
2. Every published dataset has `snapshot_id`, `content_hash`, `manifest_path`.
3. Exactly one `is_current=true` version per dataset.

## W2: Ingestion Contract and Quality Gates
### Scope
1. Standardize ingestion run lifecycle:
   - `extract -> stage -> validate -> publish -> record`.
2. Enforce manifest schema at publish time.
3. Persist quality checks (pass/warn/fail + severity) into Postgres.

### Deliverables
1. Manifest emitted per run and stored under versioned path.
2. Validation failures block publish and mark run as `failed`.
3. Watermark-based incremental pulls for prices/fundamentals.

### Acceptance
1. Re-running same watermark window is idempotent.
2. Quality failures never advance `is_current` dataset version.
3. Full lineage from source window to dataset version is queryable.

## W3: DuckDB + Query Performance
### Scope
1. Use DuckDB for read-side analytics and reproducible SQL queries.
2. Standardize partition-aware scan patterns in store/query layer.
3. Add baseline benchmark scenarios for large historical reads.

### Deliverables
1. Query recipes for common workloads (cross-sectional date slice, symbol history, factor panel).
2. Benchmarks stored in release evidence.
3. Guidance for partition tuning and compaction cadence.

### Acceptance
1. Key research queries complete within agreed SLA on reference hardware.
2. No full-table scans for common incremental workflows.

## W4: nanobook v0.9 Integration Readiness (qtrade side)
### Scope
1. Feature-gated bridge contract (`version + capability` probing).
2. Integration hooks for:
   - stop-aware backtest path,
   - GARCH forecast,
   - optimizer offload,
   - holdings offload.
3. Robust fallback to Python paths when feature missing.

### Deliverables
1. `calc.bridge` capability APIs.
2. Dispatch logic in `calc/engine.py`, `calc/nb_backtest.py`, `calc/factors/volatility.py`, `calc/sizing.py`.
3. Dual-mode tests (nanobook off/on/partial capability).

### Acceptance
1. qtrade runs green with nanobook absent.
2. qtrade runs green with partial nanobook feature set.
3. qtrade runs green with full nanobook v0.9 feature set.

## W5: Observability, Ops, and Reliability
### Scope
1. Structured run metadata and path-used diagnostics (nanobook vs python).
2. Health checks + alerting thresholds tied to production data freshness.
3. Job lease/heartbeat semantics via `job_leases`.

### Deliverables
1. Dashboard-ready metadata fields in run records.
2. Alerting on ingestion lag, quality failure, and scheduler failures.
3. Recovery runbook (retry, backfill, rollback to previous snapshot).

### Acceptance
1. On-call can identify failed dataset publish root cause within minutes.
2. Rollback to previous `dataset_versions.is_current` is documented and tested.

## W6: Security and Change Control
### Scope
1. Environment separation (`dev`, `staging`, `prod`) with explicit config boundaries.
2. Secret handling and least-privilege DB credentials.
3. Release discipline: tags, evidence, and rollback paths.

### Deliverables
1. Environment variable contract and bootstrap docs for macOS.
2. Production checklist and release template.
3. Signed-off migration order for schema and jobs.

### Acceptance
1. No secrets committed to repo.
2. Release can be reproduced from tag + manifests + DB metadata.

## Execution Phases

## P0: Freeze and Baseline (2-3 days)
1. Freeze target branch for v0.4 program work.
2. Run full tests and capture baseline benchmark numbers.
3. Confirm all data artifacts are excluded from release scope.

## P1: Metadata Backbone (3-4 days)
1. Apply `deploy/postgres/schema_v1.sql`.
2. Add startup checks validating required tables/types/indexes.
3. Wire ingestion run/version writes from pipeline code.

## P2: Manifest + Publish Discipline (4-5 days)
1. Enforce manifest generation + schema validation in publish path.
2. Add content hash + snapshot id recording.
3. Block publish on failed quality checks.

## P3: Query and Storage Hardening (3-4 days)
1. Standardize partitioning conventions and compaction workflow.
2. Add DuckDB query smoke tests and benchmark harness.
3. Document performance envelope.

## P4: nanobook Integration Layer (5-7 days)
1. Add bridge capability probing APIs.
2. Implement feature-gated dispatch points and fallback logic.
3. Expand parity + integration tests for dual-mode execution.

## P5: Ops and Production Readiness (3-4 days)
1. Add run diagnostics + alert wiring.
2. Document rollback/backfill runbooks.
3. Perform staging dress rehearsal.

## P6: Release and Cutover (2-3 days)
1. Execute full gates.
2. Tag and publish `v0.4.0`.
3. Monitor first production cycles and complete post-release review.

## Release Gates (Hard Requirements)
1. `./scripts/test_steps.sh` passes (including full suite).
2. `uv run pytest -v tests/` passes in clean environment.
3. Manifest schema validation passes for staged publish samples.
4. Ingestion lineage queries in Postgres return complete run/version trace.
5. nanobook integration tests pass in:
   - nanobook absent,
   - nanobook present but partial,
   - nanobook full (v0.9).
6. Benchmark evidence attached to release notes.

## Production Checklist
1. Postgres backup and restore test completed.
2. Dataset rollback procedure tested on staging.
3. Scheduler/health alerts verified end-to-end.
4. Secrets rotation documented.
5. Tag + manifest + metadata evidence archived.

## Ownership Model
1. qtrade session:
   - W1, W2, W3, W5, W6
   - nanobook feature-gated integration points in qtrade (W4)
2. nanobook session:
   - Rust feature implementation for v0.9 APIs
3. Joint sign-off:
   - parity, benchmark, and cutover approval

## Risks and Mitigations
1. API drift between nanobook and qtrade.
   - Mitigation: capability probing + strict adapters.
2. Hidden behavior drift in quant metrics/backtest semantics.
   - Mitigation: parity tests + explicit accepted divergences.
3. Data quality regressions in automated ingestion.
   - Mitigation: hard publish gates + quality check persistence.
4. Operational fragility during cutover.
   - Mitigation: staging dress rehearsal + rollback proof.

## Definition of Done
`qtrade v0.4.0` is complete when:
1. Data platform (Postgres + Parquet + DuckDB) is live and validated.
2. Ingestion lifecycle is deterministic, audited, and rollback-capable.
3. qtrade is nanobook v0.9-ready via capability-gated integration.
4. Full tests, parity checks, and benchmark gates are all green.
5. Production runbooks and alerting are in place and exercised.
