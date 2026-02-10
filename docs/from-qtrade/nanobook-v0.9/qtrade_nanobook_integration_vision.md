# qtrade + nanobook — Integration Vision

## The Duo

Two repos, one system. Each owns what it's best at.

- **qtrade** (Python, private) — the brain. Data pipelines, factor research, strategy manifests, experiment tracking, orchestration.
- **nanobook** (Rust, open-source) — the muscle. Matching engine, portfolio simulation, broker connectivity, pre-trade risk, execution.

They communicate through a single interface: **PyO3 Python bindings** (`pip install nanobook`). qtrade computes *what* to trade; nanobook handles *how* to trade it — fast, safe, deterministic.

---

## Why Two Repos?

| Concern | qtrade | nanobook |
|---------|--------|----------|
| **Change frequency** | Daily (research iteration) | Rarely (stable protocols) |
| **Language** | Python (Polars, statsmodels, riskfolio) | Rust (fixed-point, deterministic) |
| **Visibility** | Private (alpha, strategy IP) | Open-source (generic infra) |
| **Distribution** | `uv sync` (local) | `pip install nanobook` (PyPI wheels) |
| **Who uses it** | You | Anyone building trading systems |

The boundary is clean: **nanobook never knows your strategy**. It receives target weights and executes. It receives price series and simulates. It has no opinion about momentum vs value — that's qtrade's domain.

---

## Current State (Feb 2026)

### qtrade v1.1

```
store → pull → prep → calc → exec → track → watch → sched
  │                                    │
  └── 573 tests passing ───────────────┘
```

8 components, all working. 100 symbols (S&P 100), daily frequency. Alpaca paper trading. APScheduler orchestration. JSON experiment logs.

**Gaps vs production-grade system:**
- Static universe (hardcoded 100 symbols, no dynamic screening)
- Hardcoded strategy classes (no declarative config)
- Basic experiment tracking (JSON, no comparison UI)
- Simple orchestration (APScheduler, no retries/caching/parallelism)
- Paper trading only (Alpaca, no IBKR live)

### nanobook v0.6

```
core (LOB) + portfolio + rebalancer (IBKR CLI) + Python bindings
  │                                                    │
  └── 180+ tests (Rust + Python) ──────────────────────┘
```

Matching engine: 8M+ orders/sec, O(1) cancel, stop orders, trailing stops. Portfolio: position tracking, VWAP, Sharpe/Sortino/drawdown. Rebalancer: IBKR CLI with risk checks, limit orders, JSONL audit. Python: Exchange, Portfolio, sweep, ITCH parser.

**Gaps vs integration target:**
- IBKR client embedded in rebalancer (not reusable)
- No `Broker` trait (IBKR-specific, not generic)
- No PyO3 bindings for broker operations
- Risk engine coupled to rebalancer
- No Binance support

---

## The Integration: How They Fit

### Data Flow (Production)

```
qtrade                                          nanobook
──────                                          ────────

1. PULL data (Yahoo, FMP, FRED)
2. STORE in Parquet lake
3. PREP: calendar-align, returns
4. SCREEN universe dynamically
5. COMPUTE factors, score, rank
6. SIZE positions (riskfolio)
7. GENERATE target weights
        │
        │  weights = {"AAPL": 0.15, "MSFT": 0.12, ...}
        │
        ├──────────────────────────────────────►  8. RISK CHECK
        │                                             │
        │  ◄── pass/fail ────────────────────────────┘
        │
        ├──────────────────────────────────────►  9. EXECUTE
        │                                             │
        │                                         IBKR / Binance
        │                                         limit orders
        │                                         rate limiting
        │                                         audit trail
        │
        │  ◄── fills, positions ─────────────────────┘
        │
10. LOG to MLflow
11. MONITOR health
12. ALERT on drift
```

### Data Flow (Backtest)

```
qtrade                                          nanobook
──────                                          ────────

1. Load manifest TOML
2. Screen universe
3. Preprocess pipeline
4. Compute factor signals
5. Generate weight schedule
        │
        │  weight_schedule = {
        │    "2024-01-02": {"AAPL": 0.3, "MSFT": 0.3, ...},
        │    "2024-02-01": {"NVDA": 0.5, "META": 0.5, ...},
        │    ...
        │  }
        │  prices = {"AAPL": [(date, cents), ...], ...}
        │
        ├──────────────────────────────────────►  6. SIMULATE
        │                                             │
        │                                         Portfolio engine
        │                                         Cost model
        │                                         Position tracking
        │                                         Stop simulation
        │                                         Fixed-point math
        │
        │  ◄── returns, metrics ─────────────────────┘
        │
7. Compare in MLflow
```

### The Contract

nanobook exposes exactly three interfaces to qtrade:

```python
import nanobook

# 1. Broker — execute real trades
broker = nanobook.IbkrBroker(host, port, client_id)
broker.connect()
positions = broker.positions()
broker.submit_order(Order("AAPL", "buy", 100, "limit", 185_00))

# 2. Risk — pre-trade validation
risk = nanobook.RiskEngine(max_position_pct=0.20, max_leverage=1.0)
result = risk.check_order(order, account)

# 3. Backtest — fast portfolio simulation
result = nanobook.backtest_weights(weight_schedule, prices, cash, cost_bps)
print(result.metrics.sharpe)
```

qtrade wraps each behind its own ABC (Broker, SafetyChecker, Backtester) for testability and to support fallback implementations (Alpaca, Python safety checks, Polars backtester).

---

## What Each Repo Owns

### qtrade — Things That Change During Research

| Component | What | Why Python |
|-----------|------|-----------|
| Data lake + pipelines | Parquet I/O, incremental pulls | Polars *is* Rust under the hood |
| Factor computation | 21 factors (momentum, value, quality, vol, macro) | Changes constantly during research |
| ML alpha models | LightGBM/XGBoost factor prediction, SHAP | Research iterates fast |
| Universe screening | Market cap, volume, momentum, volatility filters | Tunable thresholds per strategy |
| Strategy manifest | TOML config driving the full pipeline | Rapid iteration, declarative |
| Walk-forward validation | Expanding-window OOS backtesting | Prevents overfitting |
| Regime detection | Macro-conditioned sizing (VIX, yield curve) | Uses FRED data already in lake |
| Risk analytics | quantstats tear sheets (40+ metrics, HTML) | Publication-quality reports |
| Preprocessing pipeline | Sequential transforms (ROC, returns, vol) | Configurable per strategy |
| Portfolio construction | Sizing, optimization | riskfolio-lib (C++/BLAS) |
| Technical indicators | RSI, MACD, Bollinger, ATR | TA-Lib (C library) |
| Experiment tracking | MLflow: params, metrics, artifacts, comparison | Strategy comparison UI |
| Orchestration | Prefect: retries, caching, parallel tasks, UI | Workflow management |
| Scheduling & monitoring | Health checks, alerts, drift detection | Ops layer |

### nanobook — Things That Are Stable Once Built

| Component | What | Why Rust |
|-----------|------|---------|
| Matching engine | LOB, price-time priority, O(1) cancel | **Done** (v0.6). Performance-critical |
| Portfolio simulation | Position tracking, VWAP, metrics | **Done** (v0.6). Fixed-point, deterministic |
| IBKR broker | TWS API connectivity, order management | Stable protocol, Rust safety |
| Binance broker | REST + WebSocket, spot trading | No good Python library for spot |
| Pre-trade risk engine | Position limits, leverage, short checks | Generic, deterministic, reusable |
| Fast backtest inner loop | Weight schedule → returns simulation | 5-10x speedup over Python loop |
| ITCH parser | NASDAQ market data replay | **Done** (v0.6). Binary parsing |

### The Rule: Rust Only Where Existing Libraries Fail

Polars IS Rust. TA-Lib is C. riskfolio-lib is C++/BLAS. These are already at native speed. Don't rewrite what works.

| Domain | Best existing tool | Rewrite in Rust? |
|--------|-------------------|:---:|
| Data wrangling | Polars | No |
| Technical indicators | TA-Lib (C) | No |
| Portfolio optimization | riskfolio-lib (C++/BLAS) | No |
| Factor analytics | statsmodels, scipy | No |
| **Broker connectivity** | ib_async (archived fork) | **Yes** |
| **Crypto exchange** | ccxt (JS-first wrapper) | **Yes** |
| **Pre-trade risk** | Nothing standalone | **Yes** |
| **Order book simulation** | Nothing in Python | **Yes** |
| **Execution engine** | Nothing unified multi-broker | **Yes** |

---

## The Seven Phases

### Phase 1: Strategy Manifest + Universe Screening (qtrade only)

**The biggest functional gap.** Currently strategies are hardcoded Python classes with a static 100-symbol universe. A production system needs: a single TOML file drives universe selection, preprocessing, signal generation, portfolio construction, backtesting.

Deliverables:
- `calc/manifest.py` — ManifestRunner: load TOML, resolve universe, preprocess, signal, backtest, log
- `prep/screener.py` — UniverseScreener: market cap → volume → momentum → volatility → price filters
- `prep/pipeline.py` — PreprocessingPipeline: sequential configurable transforms
- Sample manifests in `strategies/`

### Phase 2: MLflow Experiment Tracking (qtrade only)

Replace JSON logs with MLflow. Log params (from manifest), metrics (Sharpe, CAGR, max drawdown, IC, t-stat), artifacts (scores DataFrame, holdings). Compare strategies in the MLflow UI.

Deliverables:
- `track/mlflow_backend.py` — MLflow integration
- Modified `track/tracker.py` — dual logging (JSONL + MLflow)
- Modified `track/config.py` — `use_mlflow`, `mlflow_tracking_uri`

### Phase 3: Prefect Orchestration (qtrade only)

Replace APScheduler with Prefect for retries, input caching, parallel task execution, and dashboard. The pipeline logic (`sched/pipelines.py`) stays — Prefect wraps it with `@task` and `@flow`.

Deliverables:
- `sched/flows.py` — Prefect flows and tasks
- Modified `sched/scheduler.py` — wraps Prefect serve()
- Modified `sched/cli.py` — launches Prefect flows

### Phase 4: Broker Trait + IBKR PyO3 (nanobook v0.7)

Extract IBKR client from `rebalancer/src/ibkr/` into a new `broker/` workspace member. Define a generic `Broker` trait. Add PyO3 bindings so qtrade can call `nanobook.IbkrBroker`.

On qtrade side: `exec/nanobook_broker.py` adapter implementing qtrade's `Broker` ABC.

### Phase 5: Risk Engine (nanobook v0.8)

Extract `rebalancer/src/risk.rs` into a standalone `risk/` crate. Make `RiskEngine` configurable and generic (not tied to rebalancer's data types). Add PyO3 bindings.

On qtrade side: `exec/safety.py` delegates to `nanobook.RiskEngine` (keeps Python wrapper for backwards compatibility).

### Phase 6: Fast Backtest Bridge (nanobook v0.9)

New PyO3 function: `nanobook.backtest_weights(weight_schedule, prices, initial_cash, cost_bps)`. qtrade computes weights in Python (where factor models live), nanobook handles the simulation loop in Rust. No Python-to-Rust callbacks needed.

On qtrade side: optional fast path in `calc/engine.py` (`fast=True` kwarg).

### Phase 7: Binance Adapter (nanobook v1.0)

`BinanceBroker` implementing the `Broker` trait. Spot only. REST for orders, WebSocket for quotes (later).

On qtrade side: `exec/nanobook_binance.py` adapter + crypto price data in `pull/providers/binance.py`.

---

## Phase Dependencies

```
Phases 1-3 are qtrade-only (no nanobook dependency)
Phases 4-7 are nanobook-only or bridge work

Phase 1: Manifest + Screening ─────────────── (START HERE)
    │
Phase 2: MLflow ─────────────────────────────── (makes Phase 1 useful)
    │
Phase 3: Prefect ────────────────────────────── (orchestration upgrade)


Phase 4: Broker Trait + IBKR ────────────────── (can run in parallel with 1-3)
    │
Phase 5: Risk Engine ────────────────────────── (depends on Phase 4 trait)
    │
Phase 6: Fast Backtest ──────────────────────── (independent of 4-5)
    │
Phase 7: Binance ────────────────────────────── (depends on Phase 4 trait)
```

Two parallel tracks. qtrade Phases 1-3 can develop while nanobook Phases 4-7 develop simultaneously. The integration point is Phase 4 completion (when `pip install nanobook` gives you `IbkrBroker`).

---

## Open-Source Boundary

**nanobook publishes (open, generic):**
- Matching engine, portfolio simulator, metrics (v0.6, done)
- `Broker` trait + IBKR adapter + Binance adapter
- `RiskEngine` with configurable limits
- Fast backtest simulation function
- Python wheels on PyPI

**qtrade keeps (private, proprietary):**
- Factor models and signal logic (alpha)
- Strategy manifests and definitions
- Universe screening parameters
- Data lake contents
- Account credentials and configuration
- The thin adapter layers (`exec/nanobook_*.py`)

---

## Success Criteria

After all 7 phases:

1. **Single TOML file** defines a complete strategy: universe, screening, preprocessing, signals, portfolio, backtest, tracking
2. **Dynamic universe** — screen from 5000+ stocks down to 20-75 positions based on configurable filters
3. **MLflow dashboard** — compare strategy variants side-by-side with metrics, params, artifacts
4. **Prefect orchestration** — retries, caching, parallel tasks, monitoring dashboard
5. **IBKR live trading** — qtrade signals → nanobook execution → real fills
6. **Binance crypto** — same pipeline, different broker
7. **5-10x faster backtests** — nanobook simulates the inner loop in Rust
8. **573+ tests** — no regressions, new tests for every new component

---

## What NOT to Build

| Temptation | Why Not |
|-----------|---------|
| Data pipelines in Rust | Polars is already Rust |
| Factor scoring in Rust | scipy/statsmodels domain, changes daily |
| Portfolio optimization in Rust | riskfolio-lib is C++/BLAS |
| Technical indicators in Rust | TA-Lib is C, already fast |
| Scheduling in Rust | Not performance-critical |
| WebSocket streaming (now) | Daily frequency first, HF later |
| GUI/dashboard in Rust | Use MLflow + Prefect UIs |
| ML models in Rust | Python ML ecosystem is unmatched |
