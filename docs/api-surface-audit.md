# Public-Surface Audit per Crate

**Scope:** Current checkout, read-only, `src/` directories only.
**Purpose:** Inventory all public items across all nanobook crates to inform the "staying-0x" argument — understand what would be promised under 1.0.

## Findings

### 1. No crate enforces `missing_docs`
Doc completeness is best-effort rather than a release gate. Root `nanobook` has strong item-level docs, but public modules in `src/lib.rs` are mostly undocumented module exports.

### 2. Deprecation markers exist in Rust for old compute shims
- `src/garch.rs:103`: `garch_forecast`
- `src/optimize.rs:317`: `optimize_cvar`, `optimize_cdar`

### 3. Python exposes deprecated functions by runtime warnings, not Python-level deprecation metadata
- `garch_forecast`, `py_garch_forecast`
- `optimize_cvar`, `py_optimize_cvar`
- `optimize_cdar`, `py_optimize_cdar`
- Legacy trailing-stop `"atr"` alias is documented as deprecated in `python/src/exchange.rs:152`

### 4. `nanobook-python` version string appears stale
`python/src/lib.rs:48` sets `__version__` to `0.9.1`, while prior repo context says the package has moved beyond that. (Not verified in manifests per `src/`-only scope.)

### 5. Several public items look operational/internal but are exported as stable API
Especially in `broker` and `rebalancer`: audit logging helpers, reconciliation helpers, cache types, PID helpers, recovery routines, and broker adapter internals. These may be intentional, but they are currently part of the crate public surface.

## Inventory By Crate

### `nanobook`

**Public modules from `src/lib.rs:152`:**
`backtest_bridge`, `cv`, `garch`, `indicators`, `itch`, `multi_exchange`, `optimize`, `persistence`, `portfolio`, `stats`, `stop`.

**Primary re-exported types:**
`OrderBook`, `ValidationError`, `ApplyResult`, `Event`, `Exchange`, `Level`, `MatchResult`, `StpPolicy`, `MultiExchange`, `Order`, `OrderOwner`, `OrderStatus`, `PriceLevels`, `CancelError`, `CancelResult`, `ModifyError`, `ModifyResult`, `StopSubmitResult`, `SubmitResult`, `Side`, `BookSnapshot`, `LevelSnapshot`, `StopBook`, `StopOrder`, `StopStatus`, `TrailMethod`, `TimeInForce`, `Trade`, `OrderId`, `Price`, `Quantity`, `Symbol`, `Timestamp`, `TradeId`.

**Other public module items:**
`BacktestStopConfig`, `BacktestBridgeOptions`, `BacktestStopEvent`, `BacktestBridgeResult`, `backtest_weights`, `backtest_weights_with_options`, `time_series_split`, `garch_ewma_forecast`, `garch_forecast` (deprecated), `rsi`, `macd`, `bbands`, `atr`, `ItchMessage`, `ItchParser`, `itch_to_event`, `OptimizeError`, `OptimizerOptions`, `OptimizerResult`, `optimize_min_variance`, `optimize_min_variance_ex`, `optimize_max_sharpe`, `optimize_risk_parity`, `inverse_cvar_weights`, `inverse_cdar_weights`, `optimize_cvar` (deprecated), `optimize_cdar` (deprecated), `project_simplex`, `save_events`, `load_events`, `Portfolio`, `PortfolioSnapshot`, `CostModel`, `Metrics`, `CVaRMethod`, `Position`, `Strategy`, `BacktestResult`, `EqualWeight`, `sweep`, `sweep_strategy`, `spearman`, `quintile_spread`.

**Status:** Mostly stable, with deprecated shims clearly marked.
**Docs:** Generally complete on structs/enums/functions; module exports in `lib.rs` are not individually documented.

### `nanobook-broker`

**Public modules from `broker/src/lib.rs:9`:**
`error`, `mock`, `types`, feature-gated `ibkr`, feature-gated `binance`.

**Core public API:**
`Broker`, `BrokerError`, `Position`, `Account`, `BrokerOrder`, `BrokerSide`, `ClientOrderId`, `BrokerOrderType`, `Quote`, `BestQuote`, `OrderId`, `BrokerOrderStatus`, `OrderState`, `f64_to_fixed_checked`, `f64_cents_checked`.

**Mock API:**
`FillMode`, `RecordedOrder`, `MockBrokerBuilder`, `MockBroker`.

**IBKR API:**
`IbkrBroker`, `IbkrClient`, `ConnectionState`, `CachedOrder`, `OrderCallbackKey`, `OrderResult`, `OrderOutcome`, `encode_order`, `submit_order`, `execute_limit_order`, `cancel_order`, `reconcile_filled_order`, `reconcile_partial_fill`, `rate_limit_delay`.

**Binance API:**
`BinanceBroker`, `BinanceClient`, `BinanceOrderCache`, `ConnectionMode`, `CachedOrder`, `BalanceInfo`, `PositionInfo`, `OrderInfo`, `AccountInfo`, `DiscrepancyReport`, `Discrepancy`, `OrderResponse`, `BookTicker`, `BinanceWebSocket`, `BinanceWebSocketEvent`, `AccountUpdate`, `ExecutionReport`, audit helpers, `sign`.

**Status:** Stable by visibility, but many adapter internals are public.
**Docs:** Trait and most externally meaningful structs/functions are documented; cache/internal adapter helpers have weaker or missing docs.

### `nanobook-risk`

**Public modules from `risk/src/lib.rs:6`:**
`checks`, `config`, `error`, `report`.

**Public items:**
`RiskEngine`, `RiskConfig`, `RiskError`, `RiskReport`, `RiskCheck`, `RiskStatus`, `checks::check_batch`.

**Status:** Stable, small, coherent surface.
**Docs:** Good item-level docs on the engine/config/report types and main methods.

### `nanobook-rebalancer`

**Public modules from `rebalancer/src/lib.rs:7`:**
`audit`, `broker`, `clock_skew`, `config`, `diff`, `error`, `execution`, `kill`, `pid_file`, `recovery`, `reconcile`, `risk`, `target`.

**Public types/functions include:**
`Checkpoint`, `AuditEvent`, `AuditLog`, all audit log helpers; `BrokerGateway`; `SkewResult`, `ClockSkewDetector`; `Config`, `ConnectionConfig`, `AccountConfig`, `AccountType`, `ExecutionConfig`, `RiskConfig`, `CostConfig`, `LoggingConfig`; `RebalanceOrder`, `Action`, `CurrentPosition`, `CostEstimate`, `compute_diff`, `estimate_cost`; `RunOptions`, `CronMode`, `run`, `show_positions`, `check_status`, `run_reconcile`; `DanglingOrder`, `verify_no_dangling_orders`, `send_sigterm`, `run_kill`; PID helpers; `ReconcileReport`, `ReconcileEntry`, `reconcile`; `RecoveryAction`, `RecoveredState`, `RecoveredOrder`, `Discrepancy`, `DiscrepancyReport`, `compare_broker_state`, `reconstruct_state`, `run_recover`; `check_risk`; `TargetSpec`, `TargetMetadata`, `TargetPosition`, `Constraints`.

**Status:** Broad stable surface by visibility; likely too much CLI/ops plumbing is public.
**Docs:** Mostly documented for major items, but config subtypes and some operational glue are public with sparse docs.

### `nanobook-python`

**Exported Python classes from `python/src/lib.rs:51`:**
`IbkrBroker`, feature-gated `BinanceBroker`, `RiskEngine`, `Exchange`, `MultiExchange`, `Order`, `Event`, `SubmitResult`, `CancelResult`, `ModifyResult`, `StopSubmitResult`, `Trade`, `LevelSnapshot`, `BookSnapshot`, `BacktestResult`, `CostModel`, `Portfolio`, `Position`, `Metrics`.

**Exported Python functions:**
`compute_metrics`, `sweep_equal_weight`, `run_backtest`, `backtest_weights`, `py_backtest_weights`, feature-gated `parse_itch`, `rsi`, `macd`, `bbands`, `atr`, `spearman`, `quintile_spread`, `time_series_split`, `rolling_sharpe`, `rolling_volatility`, `capabilities`, `py_capabilities`, `garch_ewma_forecast`, `py_garch_ewma_forecast`, `garch_forecast` (deprecated by warning), `py_garch_forecast` (deprecated by warning), `optimize_min_variance`, `py_optimize_min_variance`, `optimize_max_sharpe`, `py_optimize_max_sharpe`, `optimize_risk_parity`, `py_optimize_risk_parity`, `inverse_cvar_weights`, `py_inverse_cvar_weights`, `inverse_cdar_weights`, `py_inverse_cdar_weights`, `optimize_cvar` (deprecated by warning), `py_optimize_cvar` (deprecated by warning), `optimize_cdar` (deprecated by warning), `py_optimize_cdar` (deprecated by warning).

**Status:** Stable exported module surface, with legacy aliases still present.
**Docs:** Mixed. Many `#[pyfunction]` entries and `#[pyclass]` wrappers have docstrings, but several exported wrappers lack direct docs: `capabilities`, `py_capabilities`, `parse_itch`, optimize wrappers, some result/event/order/position classes.

## Recommended Cleanup Order

1. Add `#![warn(missing_docs)]` temporarily in CI or local audit mode for each library crate to make the gaps mechanical.
2. Decide whether `rebalancer` and broker adapter internals are intended as public API; if not, reduce visibility before a 1.0-style stability promise.
3. Align Python runtime deprecations with Rust deprecation status and expose deprecation intent in docstrings.
4. Fix or verify the Python `__version__` string outside this `src/`-only audit scope.

## Implications for 1.0 Readiness

This audit reveals that nanobook's public surface is broader than a typical 1.0 candidate:

- **Stability promises:** No formal stability attributes (`#[stable]`, `#[unstable]`) are used. Stability is implied by visibility alone.
- **Documentation gaps:** Missing module-level docs and incomplete coverage on internal-but-public items.
- **Operational leakage:** CLI/ops plumbing in `rebalancer` and broker adapter internals are part of the public API.
- **Deprecation hygiene:** Python deprecations use runtime warnings instead of formal deprecation metadata.
- **Version drift:** Python `__version__` string is stale relative to actual package version.

For a 1.0 release, these gaps would need to be addressed: formal stability attributes, complete documentation, reduced visibility for internal items, and consistent deprecation hygiene. This supports the argument that nanobook should stay in 0.x while these issues are resolved and while the solo maintainer cannot underwrite indefinite 1.0 stability.
