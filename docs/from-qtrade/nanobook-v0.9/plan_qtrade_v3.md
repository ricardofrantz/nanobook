# qtrade v3 — nanobook Integration Plan

**Date:** 2026-02-09 (revised)
**Prerequisite:** qtrade v2 complete (681 tests), nanobook v0.7 + v0.8 validated
**Goal:** Replace Python computation and execution layers with nanobook's Rust engine.

---

## What nanobook v0.7 + v0.8 Provides

### v0.7 — Execution Layer

| Capability | nanobook API | Latency |
|-----------|-------------|---------|
| Schedule-driven backtest | `backtest_weights(weight_schedule, price_schedule, ...)` | GIL-released, ~0.1s per backtest |
| Custom strategy backtest | `run_backtest(strategy_fn, prices, ...)` | Python callback + Rust execution |
| Portfolio metrics | `Metrics` (Sharpe, Sortino, CAGR, Calmar, max DD, vol) | Nanoseconds |
| Portfolio tracking | `Portfolio` (VWAP, positions, equity curve, rebalance) | In-memory, O(1) per trade |
| Cost model | `CostModel(commission_bps, slippage_bps, min_trade_fee)` | Built into backtest |
| Pre-trade risk | `RiskEngine` (position %, leverage, drawdown, short exposure) | O(n) checks |
| Stop orders | Trailing stops (fixed, %, ATR-based) | Exchange-native |
| Broker: IBKR | `IbkrBroker` (connect, positions, submit, cancel, quote) | TWS blocking |
| Broker: Mock | `MockBroker` (configurable fills for tests) | Instant |
| Parameter sweep | `sweep_equal_weight()` (Rayon parallel, GIL-released) | 100 variants in seconds |
| Rebalancer CLI | `nanobook-rebalancer run target.json` (dry-run, audit) | End-to-end |
| Deterministic replay | `Exchange.replay(events)` | Exact state reconstruction |

### v0.8 — Computation Layer

| Capability | nanobook API | Replaces |
|-----------|-------------|----------|
| Technical indicators | `py_rsi`, `py_macd`, `py_bbands`, `py_atr` | ta-lib + C library |
| Extended metrics | `Metrics.cvar_95`, `.win_rate`, `.profit_factor`, `.payoff_ratio`, `.kelly` | quantstats stats |
| Rolling metrics | `py_rolling_sharpe`, `py_rolling_volatility` | quantstats rolling |
| Rank correlation | `py_spearman` (with p-value) | scipy.stats.spearmanr |
| Quintile spread | `py_quintile_spread` | numpy argsort + mean |
| Cross-validation | `py_time_series_split` | sklearn TimeSeriesSplit |
| GARCH forecast | `py_garch_forecast` (Tier 2) | arch GARCH(1,1) |

---

## Replacement Map

### FULLY REPLACED (remove Python module, use nanobook)

| qtrade Module | Lines | Replaced By | Version |
|--------------|:-----:|-------------|:-------:|
| `calc/backtest.py` | 210 | `nanobook.backtest_weights()` | v0.7 |
| `calc/metrics.py` | 72 | `nanobook.Metrics` / `py_compute_metrics()` | v0.7 |
| `exec/safety.py` | 100 | `nanobook.RiskEngine` | v0.7 |
| `exec/monitors.py` | ~120 | `nanobook.Exchange` trailing stops + `RiskEngine` drawdown | v0.7 |
| `exec/broker.py` (ABC) | 55 | `nanobook.Broker` trait | v0.7 |
| `exec/order_manager.py` | ~90 | `nanobook.Exchange` + broker submit | v0.7 |
| `calc/batch.py` | 85 | `nanobook.sweep_equal_weight()` + custom | v0.7 |

**v0.7 total: ~730 lines of Python → nanobook FFI calls.**

### REPLACED BY v0.8 (delegate computation to nanobook, keep orchestration in Python)

| qtrade Module | What Moves to Rust | nanobook API | Lines Changed |
|--------------|-------------------|-------------|:-------------:|
| `calc/factors/technical.py` | RSI, MACD, BBands, ATR calls | `py_rsi`, `py_macd`, `py_bbands`, `py_atr` | ~40 |
| `calc/stops.py` | ATR computation for stop levels | `py_atr` | ~10 |
| `calc/validation.py` | Spearman IC + quintile spread + numpy ops | `py_spearman`, `py_quintile_spread` | ~30 |
| `calc/walkforward.py` | TimeSeriesSplit fold generation | `py_time_series_split` | ~10 |
| `calc/analytics.py` | CVaR, win_rate, profit_factor, payoff, Kelly, rolling Sharpe/vol | `Metrics` fields + `py_rolling_sharpe/volatility` | ~40 |
| `calc/factors/volatility.py` | GARCH(1,1) forecast (Tier 2) | `py_garch_forecast` | ~15 |

**v0.8 total: ~145 lines changed in Python (replace library calls with nanobook calls).**

### PARTIALLY REPLACED (keep Python module, delegate hot path to nanobook)

| qtrade Module | What Stays (Python) | What Moves (Rust) |
|--------------|--------------------|--------------------|
| `calc/engine.py` | `run()`, `run_manifest()` orchestration, data loading | Backtest call → `nanobook.backtest_weights()` |
| `calc/sweep.py` | `generate_variants()`, `_set_nested()` | Batch execution → `nanobook.sweep_equal_weight()` |
| `calc/evolve.py` | Evolution logic (breed, select, mutate) | Inner backtest loop → nanobook |
| `exec/rebalancer.py` | Sell-first-then-buy orchestration | Broker calls → nanobook broker, safety → nanobook RiskEngine |
| `track/tca.py` | TCA analysis logic | Fill data from nanobook `Trade` objects |

### NOT REPLACED (stays Python, no nanobook overlap)

| qtrade Module | Reason |
|--------------|--------|
| `store/*` (lake, storage, query, versions) | Data lake is Polars+DuckDB+Parquet — already Rust/C++ core |
| `pull/*` (providers, puller, normalize) | Data acquisition (HTTP APIs) — I/O bound |
| `prep/*` (calendar, returns, validate, universe) | Data prep is Polars-native — already fast |
| `calc/factors/momentum.py` | Polars column ops — fast enough |
| `calc/factors/value.py` | Polars column ops |
| `calc/factors/quality.py` | Polars column ops |
| `calc/factors/growth.py` | Polars column ops |
| `calc/factors/macro.py` | Polars column ops |
| `calc/scoring.py` | Z-score, composite scoring — Polars ops |
| `calc/sizing.py` | riskfolio-lib optimization (HRP, MinVar) — complex solver |
| `calc/manifest.py` | Pydantic TOML manifest — config only |
| `calc/regime.py` | Macro signal classification — simple logic |
| `calc/ml/*` | ML pipeline (LightGBM, XGBoost, SHAP) — own C++ backends |
| `track/*` (tracker, drift, mlflow_backend) | Experiment tracking — I/O + logging |
| `watch/*` (checks, alerter, watchdog) | Health monitoring — system checks |
| `sched/*` (pipelines, scheduler, cli, flows) | Scheduling — APScheduler/Prefect orchestration |
| `exec/config.py` | Pydantic config — stays |
| `exec/alpaca.py` | Alpaca paper trading — keep alongside nanobook IBKR |

---

## Dependency Changes

### Packages REMOVED from `pyproject.toml`

| Package | Current Use | Replacement | Fully Gone from env? |
|---------|------------|-------------|:--------------------:|
| **statsmodels** (>=0.14) | Never imported (dead dep) | N/A — just remove | Possibly (quantstats may pull transitively) |
| **ta-lib** (>=0.4) | RSI, MACD, BBands, ATR | `nanobook::indicators` | **Yes** — no transitive deps, C lib eliminated |
| **scikit-learn** (>=1.3) | TimeSeriesSplit only | `nanobook::cv` | No — riskfolio-lib, shap pull transitively |

### Packages REMOVED (Tier 2)

| Package | Current Use | Replacement | Fully Gone? |
|---------|------------|-------------|:-----------:|
| **arch** (>=7.0) | GARCH(1,1) forecast | `nanobook::garch` | **Yes** — no transitive deps |

### Packages ADDED

| Package | Purpose |
|---------|---------|
| **nanobook** (>=0.8) | Rust engine — backtest, metrics, risk, broker, stops, indicators, stats, CV |

### Packages with REDUCED usage (but kept)

| Package | Before (v2) | After (v3) | Why Keep |
|---------|-------------|------------|----------|
| **numpy** | 4 files (metrics, backtest, validation, ml/explain) | 1 file (ml/explain.py for SHAP) | Transitive dep of polars, scipy, ML libs |
| **scipy** | 1 file (validation.py — spearmanr) | 0 direct imports | Transitive dep of riskfolio-lib, exchange-calendars |
| **quantstats** | 13 call sites (metrics + tear sheets) | 1 call site (HTML tear sheet only) | `qs.reports.html()` for presentation |

### Net Change

| Metric | v2 (current) | v3 (after all phases) |
|--------|:------------:|:---------------------:|
| Packages in pyproject.toml | 24 | **22** (-statsmodels, -ta-lib, -sklearn, +nanobook) |
| C library requirements | 1 (ta-lib) | **0** |
| Direct numpy imports | 4 files | **1 file** |
| Direct scipy imports | 1 file | **0 files** |
| Direct sklearn imports | 1 file | **0 files** |
| Direct quantstats metric calls | 12 | **0** (1 HTML call remains) |

---

## Architecture: Before vs After

### v2 (Current) — Python Everywhere

```
Python Strategy Logic
    ↓
calc/engine.py  →  calc/backtest.py (Python loop + numpy)
                       ↓
                   calc/metrics.py (numpy)
    ↓
calc/factors/technical.py (ta-lib C library)
calc/validation.py (scipy Spearman + numpy)
calc/walkforward.py (sklearn TimeSeriesSplit)
calc/analytics.py (quantstats metrics)
calc/factors/volatility.py (arch GARCH)
    ↓
exec/safety.py (Python checks)  →  exec/rebalancer.py  →  exec/alpaca.py (Alpaca paper)
exec/monitors.py (Python stop tracking)
```

### v3 (Target) — Rust Engine

```
Python Intelligence Layer (unchanged)
├── store/* — Polars + DuckDB + Parquet data lake
├── pull/* — HTTP data acquisition (yfinance, FMP, FRED)
├── prep/* — Calendar alignment, returns, universe (Polars-native)
├── calc/factors/{momentum,value,quality,growth,macro}.py (Polars column ops)
├── calc/scoring.py — Z-score composites (Polars)
├── calc/sizing.py — Portfolio optimization (riskfolio-lib)
├── calc/regime.py — Macro regime classification
├── calc/ml/* — ML alpha (LightGBM, XGBoost, SHAP)
├── track/* — Experiment tracking (JSONL, MLflow)
├── watch/* — Health monitoring (psutil, exchange-calendars)
└── sched/* — Scheduling (APScheduler, Prefect, typer CLI)

nanobook Rust Engine (v0.7 + v0.8)
├── backtest_weights() — Vectorized backtest (v0.7)
├── Metrics — Sharpe, Sortino, CAGR, Calmar, maxDD, vol,
│             CVaR, win_rate, profit_factor, payoff, Kelly (v0.7+v0.8)
├── RiskEngine — Pre-trade risk checks (v0.7)
├── Exchange — Trailing stops, order management (v0.7)
├── IbkrBroker / MockBroker — Broker interface (v0.7)
├── sweep_equal_weight() — Parallel sweep (v0.7)
├── py_rsi/macd/bbands/atr — Technical indicators (v0.8)
├── py_spearman/quintile_spread — Statistics (v0.8)
├── py_time_series_split — Cross-validation splits (v0.8)
├── py_rolling_sharpe/volatility — Rolling metrics (v0.8)
└── py_garch_forecast — GARCH(1,1) volatility (v0.8, Tier 2)
```

**Key principle:** Python decides WHAT to trade (signals, weights, ML).
Rust decides HOW to trade (execution, risk, metrics) AND computes all numerical indicators/statistics.

---

## Implementation Phases

### v0.7 Integration (Phases 0–6)

#### Phase 0: Add nanobook dependency + bridge types (~100 lines)

**Goal:** Wire nanobook into the project, create type converters.

1. Add `nanobook>=0.8` to `pyproject.toml` dependencies
2. Remove `statsmodels>=0.14` (dead dependency)
3. Create `calc/bridge.py` — conversion layer:
   - `weights_to_schedule(weights_by_date, prices_df)` → nanobook `weight_schedule` + `price_schedule`
   - `metrics_from_nanobook(nb_metrics)` → existing `BacktestResult` metrics fields
   - `prices_to_cents(prices_df)` → convert float dollars to int cents for nanobook

**Tests:** `test_bridge.py` — round-trip conversion, cent precision, edge cases

#### Phase 1: Replace `calc/backtest.py` with nanobook (~150 lines changed)

**Goal:** Swap the inner backtest loop — highest-impact change.

1. Modify `calc/engine.py::backtest()`:
   - Build `weight_schedule` from rebalance dates × `score_and_select` → `size_fn`
   - Build `price_schedule` from lake data (convert adj_close to cents)
   - Call `nanobook.backtest_weights(weight_schedule, price_schedule, initial_cash, cost_bps, ...)`
   - Convert `result['metrics']` back to `BacktestResult`
2. Keep `calc/backtest.py` as `calc/_backtest_legacy.py` (fallback) during transition
3. Stop simulation: nanobook trailing stops handle this natively

**Tests:** All existing `test_calc_backtest.py` must pass. Add A/B comparison test (legacy vs nanobook).

#### Phase 2: Replace `calc/metrics.py` with nanobook (~30 lines changed)

**Goal:** Use `nanobook.py_compute_metrics()` for all metric computation.

1. Rewrite `calc/metrics.py` functions to delegate to nanobook:
   ```python
   def sharpe_ratio(returns, *, risk_free=0.0, periods=252):
       m = nanobook.py_compute_metrics(returns.to_list(), periods, risk_free)
       return m.sharpe
   ```
2. Add Sortino and Calmar (free from nanobook, not in v2)

**Tests:** Existing `test_calc_metrics.py` pass. Add precision comparison tests.

#### Phase 3: Replace `exec/safety.py` with nanobook RiskEngine (~80 lines changed)

**Goal:** Use nanobook's pre-trade risk validation.

1. Create thin wrapper in `exec/safety.py`:
   ```python
   class SafetyChecker:
       def __init__(self, config):
           self._risk = nanobook.RiskEngine(
               max_position_pct=config.max_position_pct,
               max_trade_usd=config.max_order_value,
               max_drawdown_pct=config.max_drawdown_pct,
               allow_short=False,
           )
   ```
2. Map `check_order()` to `self._risk.check_order()`, convert result

**Tests:** All existing `test_exec_safety.py` pass with nanobook backend.

#### Phase 4: Replace `exec/monitors.py` stop tracking (~60 lines changed)

**Goal:** Use nanobook's trailing stop orders instead of Python stop monitor.

1. `StopLossMonitor` → nanobook `Exchange.submit_trailing_stop_market()` with ATR method
2. `DrawdownMonitor` → nanobook `RiskEngine.max_drawdown_pct` (already in Phase 3)
3. Stop data from `calc/stops.py` feeds into nanobook stop submission

**Tests:** Existing stop monitor tests rewritten to use nanobook Exchange.

#### Phase 5: Add IBKR broker via nanobook (~60 lines new)

**Goal:** Enable live trading through nanobook's IBKR broker alongside existing Alpaca paper.

1. Create `exec/ibkr.py` — thin wrapper around `nanobook.IbkrBroker`
2. Implement same `Broker` ABC as `AlpacaBroker`
3. Add `broker_type: Literal["alpaca", "ibkr"] = "alpaca"` to `ExecConfig`
4. Update `sched/cli.py::_build_pipeline_kwargs()` to select broker by config

**Tests:** `test_exec_ibkr.py` with `nanobook.MockBroker`.

#### Phase 6: Turbocharge parameter sweep (~40 lines changed)

**Goal:** Use nanobook's parallel sweep for `calc/batch.py` and `calc/evolve.py`.

1. `batch_backtest()` → loop of `nanobook.backtest_weights()` calls (each GIL-released)
2. For equal-weight variants → use `nanobook.sweep_equal_weight()` (Rayon parallel)
3. `StrategyEvolver` inner loop → nanobook backtests

**Tests:** Existing `test_sweep.py` + `test_evolve.py` pass.

---

### v0.8 Integration (Phases 7–11)

**Prerequisite:** nanobook v0.8 validated (all reference tests + property tests passing).
See `plan_nanobook_v0.8.md` for the nanobook-side implementation plan.

#### Phase 7: Replace technical indicators (~50 lines changed)

**Goal:** Eliminate ta-lib (and its C library dependency) from qtrade.

**Files changed:**
- `calc/factors/technical.py` — 4 ta-lib calls → nanobook
- `calc/stops.py` — 1 ta-lib ATR call → nanobook

**Before → After:**
```python
# calc/factors/technical.py — BEFORE
import talib
rsi = talib.RSI(closes, timeperiod=14)
macd, signal, _ = talib.MACD(closes, fastperiod=12, slowperiod=26, signalperiod=9)
upper, middle, lower = talib.BBANDS(closes, timeperiod=20, nbdevup=2, nbdevdn=2)
atr = talib.ATR(high, low, close, timeperiod=14)

# calc/factors/technical.py — AFTER
import nanobook
rsi = nanobook.py_rsi(closes.tolist(), 14)
macd, signal, _ = nanobook.py_macd(closes.tolist(), 12, 26, 9)
upper, middle, lower = nanobook.py_bbands(closes.tolist(), 20, 2.0, 2.0)
atr = nanobook.py_atr(high.tolist(), low.tolist(), close.tolist(), 14)
```

**After this phase:**
- Remove `ta-lib>=0.4` from `pyproject.toml`
- Remove `brew install ta-lib` from setup docs
- No more C library build dependency

**Tests:** Existing `test_calc_factors.py` tests pass. Add A/B parity test (ta-lib vs nanobook, atol=1e-10).

#### Phase 8: Replace statistics + cross-validation (~40 lines changed)

**Goal:** Eliminate direct scipy and sklearn imports from qtrade source.

**Files changed:**
- `calc/validation.py` — scipy.stats.spearmanr + numpy ops → nanobook
- `calc/walkforward.py` — sklearn TimeSeriesSplit → nanobook

**Before → After:**
```python
# calc/validation.py — BEFORE
from scipy import stats
import numpy as np
ic, p_value = stats.spearmanr(scores, rets)
t_stat = ic * np.sqrt((n - 2) / (1 - ic**2))
sorted_idx = np.argsort(scores)
bottom_mean = np.mean(rets[sorted_idx[:quintile]])
top_mean = np.mean(rets[sorted_idx[-quintile:]])
alpha = top_mean - bottom_mean

# calc/validation.py — AFTER
import nanobook
ic, p_value = nanobook.py_spearman(scores.tolist(), rets.tolist())
t_stat = ic * ((n - 2) / (1 - ic**2)) ** 0.5  # pure Python math
alpha = nanobook.py_quintile_spread(scores.tolist(), rets.tolist(), 5)
```

```python
# calc/walkforward.py — BEFORE
from sklearn.model_selection import TimeSeriesSplit
tscv = TimeSeriesSplit(n_splits=self.n_splits)
for train_idx, test_idx in tscv.split(rebalance_dates):

# calc/walkforward.py — AFTER
import nanobook
splits = nanobook.py_time_series_split(len(rebalance_dates), self.n_splits)
for train_idx, test_idx in splits:
```

**After this phase:**
- Remove `scikit-learn>=1.3` from `pyproject.toml`
- Zero `from scipy import stats` in qtrade source
- Zero `import numpy as np` in calc/validation.py

**Tests:** Existing `test_calc_validation.py` + `test_calc_walkforward.py` pass.

#### Phase 9: Replace analytics metrics (~40 lines changed)

**Goal:** Use nanobook extended metrics instead of quantstats for all metric computation.

**Files changed:**
- `calc/analytics.py` — 12 quantstats metric calls → nanobook

**Before → After:**
```python
# calc/analytics.py — BEFORE
import quantstats as qs
cvar = qs.stats.cvar(ret_pd)
win_rate = qs.stats.win_rate(ret_pd)
profit_factor = qs.stats.profit_factor(ret_pd)
payoff_ratio = qs.stats.payoff_ratio(ret_pd)
kelly = qs.stats.kelly_criterion(ret_pd)
rolling_sharpe = qs.stats.rolling_sharpe(ret_pd, rolling_period=window)
rolling_vol = qs.stats.rolling_volatility(ret_pd, rolling_period=window)

# calc/analytics.py — AFTER
import nanobook
m = nanobook.py_compute_metrics(returns, 252.0, 0.0)
cvar = m.cvar_95
win_rate = m.win_rate
profit_factor = m.profit_factor
payoff_ratio = m.payoff_ratio
kelly = m.kelly
rolling_sharpe = nanobook.py_rolling_sharpe(returns, window, 252)
rolling_vol = nanobook.py_rolling_volatility(returns, window, 252)
```

**After this phase:**
- quantstats usage reduced to 1 call: `qs.reports.html()` (HTML tear sheets only)
- All 12 metric computations now in Rust

**Tests:** Existing `test_calc_analytics.py` pass. Add A/B parity test (quantstats vs nanobook).

#### Phase 10: Replace GARCH — Tier 2 (~15 lines changed)

**Goal:** Eliminate arch dependency.

**Prerequisite:** nanobook v0.8 GARCH module implemented and validated.

**Files changed:**
- `calc/factors/volatility.py` — arch GARCH(1,1) → nanobook

**Before → After:**
```python
# BEFORE
from arch import arch_model
scaled = rets.to_numpy() * 100
model = arch_model(scaled, vol="Garch", p=1, q=1, mean="Zero")
result = model.fit(disp="off")
forecast = result.forecast(horizon=1)
cond_var = forecast.variance.iloc[-1, 0]

# AFTER
import nanobook
result = nanobook.py_garch_forecast(rets.tolist(), 1, 1)
cond_var = result.forecast_variance
```

**After this phase:**
- Remove `arch>=7.0` from `pyproject.toml`
- arch fully eliminated (no transitive deps)

**Tests:** Existing `test_calc_factors_volatility.py` pass. A/B parity test (arch vs nanobook, atol=1e-4).

---

### Cleanup Phase (Phase 11)

#### Phase 11: Dependency cleanup + verification

**Goal:** Remove dead dependencies, verify zero direct imports, update docs.

1. **pyproject.toml changes:**
   - Remove: `statsmodels>=0.14`, `ta-lib>=0.4`, `scikit-learn>=1.3`
   - Remove (Tier 2): `arch>=7.0`
   - Add: `nanobook>=0.8`
   - Keep (reduced): `quantstats` (HTML only), `numpy` (ml/explain.py only), `scipy` (transitive)

2. **Verify zero direct imports:**
   ```bash
   # These should return NO matches in qtrade source (excluding tests):
   rg "import talib" --type py
   rg "from scipy import" --type py
   rg "from sklearn" --type py
   rg "from arch import" --type py  # Tier 2
   rg "import numpy" --type py      # Should only match calc/ml/explain.py
   ```

3. **Update documentation:**
   - README: remove `brew install ta-lib` from setup
   - pyproject.toml: update package description
   - CHANGELOG: document v3 migration

4. **Run full test suite:** `uv run pytest` — all 681+ tests must pass

---

## Migration Strategy

```
v0.7 Integration:
Phase 0  (bridge):       Add nanobook dep, conversion helpers         → tests pass
Phase 1  (backtest):     Swap backtest core                           → tests pass, 10x faster
Phase 2  (metrics):      Swap metrics (base + extended)               → tests pass
Phase 3  (safety):       Swap pre-trade risk                          → tests pass
Phase 4  (stops):        Swap stop monitoring                         → tests pass
Phase 5  (IBKR):         Add IBKR broker                              → new capability
Phase 6  (sweep):        Parallel sweep                               → tests pass, Nx faster

v0.8 Integration:
Phase 7  (indicators):   ta-lib → nanobook (eliminates C dep!)        → tests pass
Phase 8  (stats+cv):     scipy + sklearn → nanobook                   → tests pass
Phase 9  (analytics):    quantstats metrics → nanobook                → tests pass
Phase 10 (garch):        arch → nanobook (Tier 2)                     → tests pass

Cleanup:
Phase 11 (deps):         Remove dead deps, verify, update docs        → all tests pass
```

Each phase is independently committable with all tests passing. No big-bang migration.

---

## Expected Performance Gains

| Operation | v2 (Python) | v3 (nanobook) | Speedup |
|-----------|:-----------:|:-------------:|:-------:|
| Single backtest (100 symbols, 5yr) | ~2s | ~0.1s | **20x** |
| Metrics computation | ~1ms | ~1μs | **1000x** |
| Pre-trade risk check | ~0.1ms | ~0.01ms | **10x** |
| 108-variant sweep | ~4.5 min | ~12s | **22x** |
| 2500-variant evolution | ~21 min | ~50s | **25x** |
| RSI/MACD/BBANDS/ATR (1000 bars) | ~0.5ms (C) | ~0.05ms (Rust) | **10x** |
| Spearman correlation (100 pairs) | ~0.1ms | ~0.01ms | **10x** |

---

## Risk Assessment

| Risk | Mitigation |
|------|-----------|
| Cent-precision mismatch | Bridge layer converts float dollars ↔ int cents; add round-trip tests |
| nanobook API changes | Pin `>=0.8,<0.9`; bridge layer absorbs API drift |
| Alpaca paper still needed | Keep `exec/alpaca.py` — nanobook adds IBKR, doesn't replace Alpaca |
| Test parity | Every phase: existing tests must pass with new backend before removing old code |
| Fixed-point edge cases | nanobook uses `Price(i64)` in cents — verify no overflow for prices > $21M |
| v0.8 numerical parity | Reference tests in nanobook validate vs original Python libs (atol=1e-10) |
| ta-lib C lib removal | Phase 7 is reversible — keep ta-lib in dev deps until fully validated |

---

## Summary

| Metric | Before (v2) | After (v3) |
|--------|:-----------:|:----------:|
| Runtime deps | 24 packages | **22 packages** (+nanobook, -statsmodels, -ta-lib, -sklearn) |
| C library deps | 1 (ta-lib) | **0** |
| Python lines replaced/changed | — | ~730 replaced + ~145 changed = **~875 lines** |
| Direct numpy imports | 4 files | **1 file** (ml/explain.py) |
| Direct scipy imports | 1 file | **0 files** |
| Direct sklearn imports | 1 file | **0 files** |
| quantstats metric calls | 12 | **0** (1 HTML call remains) |
| New capability | — | IBKR live trading, extended metrics, parallel sweep, no C deps |
| Backtest speed | ~2s | ~0.1s |
| Sweep speed (108 variants) | 4.5 min | 12s |

**Bottom line:** v3 replaces the **execution + measurement + computation layers** with nanobook's Rust engine. The integration is surgical — 11 phases, each independently testable. The biggest UX win is eliminating the ta-lib C library dependency. The biggest performance win is the 20x backtest speedup. Python retains the **intelligence layer** (factors, scoring, sizing, ML, regime) and the **infrastructure layer** (data lake, scheduling, tracking, alerting).
