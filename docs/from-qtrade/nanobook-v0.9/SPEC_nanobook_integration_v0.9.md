# SPEC_nanobook_integration_v0.9

## Purpose
Define the qtrade-side integration contract for nanobook v0.9 so rollout is fast, safe, and reversible.

## Non-Goals
- Rewriting strategy logic in Rust.
- Removing Python fallback paths in v0.9.
- Replacing data orchestration (pull/prep/sched/track/watch).

## Target Outcome
1. qtrade automatically uses nanobook v0.9 features when present.
2. Missing nanobook features fall back to existing Python paths without behavior regressions.
3. Parity and edge-case behavior are explicit and test-guarded.

## Capability Model
qtrade must not assume "nanobook installed" implies all v0.9 features are present.

### Bridge APIs (qtrade)
- `has_nanobook() -> bool`
- `nanobook_version() -> str | None`
- `has_nanobook_feature(name: str) -> bool`

### Canonical Feature Names
- `backtest_stops`
- `garch_forecast`
- `optimize_min_variance`
- `optimize_max_sharpe`
- `optimize_risk_parity`
- `optimize_cvar`
- `optimize_cdar`
- `backtest_holdings`

## Integration Changes by Module

### 1) `calc/bridge.py`
- Add version/capability probing.
- Capability probe checks both explicit `nanobook.py_capabilities()` (if present) and symbol-based fallback.
- Cache probe results to avoid repeated import/inspection overhead.

### 2) `calc/engine.py`
- Nanobook backtest path should allow stop simulation when `backtest_stops` is available.
- If `simulate_stops=True` and feature missing, log/route to Python path.

### 3) `calc/nb_backtest.py`
- Accept stop config pass-through to nanobook call.
- Consume holdings from nanobook result when `backtest_holdings` is available.
- Keep Python holdings reconstruction only as fallback.

### 4) `calc/factors/volatility.py`
- Dispatch `_garch_forecast()` to `nanobook.py_garch_forecast` when `garch_forecast` is available.
- Preserve `arch` fallback and existing failure behavior.

### 5) `calc/sizing.py`
- Add nanobook optimizer dispatch for non-equal-weight methods.
- Preserve long-only clamp + re-normalization invariants.
- Preserve equal-weight fallback on invalid optimizer output.

## Behavioral Contracts

### Stops
- Entry/exit semantics must match current qtrade assumptions:
  - stop checks are intraperiod
  - first breach triggers exit
  - cost handling remains unchanged unless explicitly revised

### Metrics
- Keep current guarding behavior for non-finite/degenerate values.
- Batch and scalar metric paths must remain consistent.

### Sizing
- Returned weights must be finite, non-negative, and sum to 1.0 within tolerance.

## Testing Requirements

### Unit
- Bridge capability/version tests.
- Feature-gated dispatch tests for each module.
- Fallback tests when feature absent but nanobook installed.

### Parity
- Extend `tests/test_nanobook_parity.py` to include new v0.9 features.
- Tighten tolerances where formula parity is expected.
- Keep explicit comments for intentional divergences only.

### Integration
- Backtest with and without stops in nanobook path.
- Sizing methods with representative returns windows.
- GARCH factor on stable and noisy datasets.

## Rollout Strategy
1. Merge bridge capability layer first.
2. Enable each feature behind capability checks.
3. Keep fallback paths for at least one release cycle.
4. Promote to default nanobook path after parity/benchmark gates pass.

## Failure Policy
- If nanobook import fails: use Python path.
- If specific feature missing: use module-level Python fallback.
- If nanobook call errors at runtime: raise typed error only when fallback is unsafe; otherwise fallback and mark run metadata.

## Observability
Record per run (track/metadata):
- nanobook version
- features detected
- which path used per subsystem (`metrics`, `backtest`, `stops`, `sizing`, `garch`)

## Acceptance Criteria
1. Full test suite passes with nanobook disabled.
2. Full test suite passes with nanobook v0.9 enabled.
3. New feature-specific tests pass in both feature-present and feature-missing modes.
4. No undocumented divergence in core metrics/backtest outputs.
5. Benchmark suite shows net speedup in nanobook-enabled mode.
