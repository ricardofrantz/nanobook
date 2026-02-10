# plan_nanobook_v0.9

## Goal
Deliver `nanobook v0.9` as the primary compute engine for qtrade so the Python layer becomes orchestration + data plumbing, while quant-critical paths are Rust-native, faster, and behaviorally consistent.

## Scope Summary
- Add missing nanobook features currently blocking full offload.
- Tighten parity between nanobook and Python fallback semantics.
- Simplify qtrade by replacing Python heavy paths with nanobook capability-gated dispatch.
- Preserve safe fallback behavior when nanobook or specific features are unavailable.

## Current Gaps (from qtrade)
1. Backtest stop simulation is not supported in nanobook path (`calc/engine.py` nanobook note).
2. GARCH forecast is still Python-only (`calc/factors/volatility.py`).
3. Portfolio optimization still relies on pandas+riskfolio in Python (`calc/sizing.py`).
4. Nanobook backtest still reconstructs holdings in Python (`calc/nb_backtest.py`).
5. Known parity divergences are documented in tests (`tests/test_nanobook_parity.py`) and should be reduced.

## v0.9 Feature Set (nanobook)

### 1) Stop-Aware Backtest Engine
- Extend `py_backtest_weights(...)` with optional stop configuration.
- Support:
  - fixed stop
  - ATR multiple stop
  - trailing stop
- Return deterministic stop-trigger metadata (trigger index/date, exit price, reason).
- Preserve current default behavior when stops are disabled.

### 2) GARCH Forecast API
- Add `py_garch_forecast(returns: list[float], p: int=1, q: int=1, mean: str=\"zero\") -> float`.
- Use robust convergence handling and deterministic fallback on failures.
- Match qtrade convention (volatility-oriented output expected by factor code).

### 3) Portfolio Optimizer APIs
- Add long-only optimizers:
  - `py_optimize_min_variance`
  - `py_optimize_max_sharpe`
  - `py_optimize_risk_parity`
  - `py_optimize_cvar`
  - `py_optimize_cdar`
- Input: dense returns matrix + symbol list.
- Output: `dict[str, float]` normalized to sum ~1.0.
- Guarantee non-negative weights in API contract.

### 4) Holdings in Backtest Result
- Backtest return payload should include per-period per-symbol holdings and symbol returns.
- Remove Python-side holdings reconstruction in qtrade.

### 5) Capability/Version Contract
- Expose stable feature introspection (`__version__`, capability flags).
- Keep backward compatibility where possible, fail clearly where not.

## qtrade Integration Plan

### Milestone A: Capability-Gated Bridge (qtrade)
1. Add `nanobook_version()` and `has_nanobook_feature(name)` in `calc.bridge`.
2. Centralize feature-name → expected nanobook symbol mapping.
3. Use feature gates before calling new v0.9 APIs.

### Milestone B: Stop Simulation Offload
1. Wire `Engine.backtest(use_nanobook=True, simulate_stops=True)` to stop-aware nanobook path.
2. Keep Python path as fallback for missing nanobook stop feature.
3. Remove/retire nanobook stop limitation comment once parity is validated.

### Milestone C: GARCH Offload
1. Update `_garch_forecast()` to dispatch to `nanobook.py_garch_forecast` when available.
2. Preserve current `arch` fallback behavior.
3. Add parity tests and convergence edge-case tests.

### Milestone D: Optimizer Offload
1. Add nanobook-backed optimizer path in `calc/sizing.py`.
2. Keep riskfolio as fallback and for regression comparison.
3. Preserve existing fallback to equal weight when optimizer output is degenerate.

### Milestone E: Holdings Offload
1. Update `calc/nb_backtest.py` to consume holdings returned by nanobook directly.
2. Remove `_reconstruct_holdings` once tests prove parity.

### Milestone F: Parity Tightening
1. Reduce broad tolerance windows where feasible.
2. Decide and codify single authoritative formulas for:
  - Sortino downside deviation
  - Calmar edge cases
  - single-element returns
3. Keep explicit documented divergence only where intentional.

## Acceptance Criteria (Release Gate)
1. All existing test suites pass with nanobook enabled/disabled.
2. New nanobook features are capability-gated and do not break fallback mode.
3. End-to-end backtest with `use_nanobook=True` supports stop simulation and produces valid holdings.
4. GARCH factor runs nanobook path when available; fallback remains green.
5. Sizing methods run nanobook path and preserve long-only invariants.
6. `tests/test_nanobook_parity.py` passes with reduced divergence and explicit documented exceptions only.

## Performance Targets
1. `compute_all` and analytics remain >= current v0.8 performance.
2. Backtest (5000+ periods equivalent workload) shows measurable speedup vs current mixed path.
3. Sizing workflows avoid pandas conversion on nanobook path.

## API Contract Draft (for nanobook v0.9)
1. `py_backtest_weights(..., stop_cfg: dict | None = None) -> dict`
2. `py_garch_forecast(returns, p=1, q=1, mean=\"zero\") -> float`
3. `py_optimize_min_variance(returns_matrix, symbols) -> dict`
4. `py_optimize_max_sharpe(returns_matrix, symbols, risk_free=0.0) -> dict`
5. `py_optimize_risk_parity(returns_matrix, symbols) -> dict`
6. `py_optimize_cvar(returns_matrix, symbols, alpha=0.95) -> dict`
7. `py_optimize_cdar(returns_matrix, symbols, alpha=0.95) -> dict`

## Risk Register
1. Formula drift between Python and Rust can cause strategy behavior changes.
   - Mitigation: parity tests + explicit spec docs.
2. Optimizer numerical stability on sparse or short return windows.
   - Mitigation: strict input guards + deterministic fallback.
3. Stop simulation semantics mismatch (intraperiod assumptions).
   - Mitigation: canonical spec and fixture-based tests.
4. Backward compatibility concerns across qtrade sessions.
   - Mitigation: capability-gated dispatch + explicit errors.

## Execution Order (Recommended)
1. Bridge capability/version contract (qtrade).
2. nanobook stop-aware backtest + qtrade integration.
3. nanobook GARCH + qtrade integration.
4. nanobook optimizers + qtrade integration.
5. holdings offload + cleanup.
6. parity tightening + benchmark pass.

## Deliverables Checklist
- [ ] nanobook v0.9 features implemented and documented.
- [ ] qtrade integration PRs merged for each milestone.
- [ ] Updated parity + benchmark reports attached to release notes.
- [ ] Release tag cut: `nanobook v0.9.0`, `qtrade v0.9.0` (or next qtrade minor per release policy).
