# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.8.0] - 2026-02-09

### Added

- **Analytics module**: Technical indicators replacing ta-lib dependency
  - `rsi()` — Relative Strength Index (14-period default)
  - `macd()` — Moving Average Convergence Divergence with signal line
  - `bollinger_bands()` — Bollinger Bands (mean ± 2 std)
  - `atr()` — Average True Range for volatility measurement
- **Statistics module**: Statistical functions replacing scipy
  - `spearman()` — Spearman rank correlation with p-value (custom beta implementation)
  - `quintile_spread()` — Cross-sectional quintile spread for factor analysis
  - `rank_data()` — Fractional ranking with tie handling
- **Time-series cross-validation**: `time_series_split()` replacing sklearn
  - Expanding window splits with configurable train/test sizes
  - Python bindings for sklearn-compatible usage
- **Extended portfolio metrics**:
  - `cvar` — Conditional Value at Risk (parametric, 95% default)
  - `win_rate` — Percentage of positive returns
  - `profit_factor` — Ratio of gross profits to gross losses
  - `payoff_ratio` — Average win divided by average loss
  - `kelly_criterion` — Optimal Kelly fraction for position sizing
  - `rolling_sharpe()` — Rolling Sharpe ratio (252-day window default)
  - `rolling_volatility()` — Rolling annualized volatility
- **Python bindings**: All new functions exposed via PyO3 with NumPy integration
- **Property tests**: Hypothesis-based tests for indicators, stats, CV (44 new tests)
- **Reference tests**: Validation against ta-lib, scipy, sklearn

### Changed

- **Performance optimizations**:
  - Rolling metrics use O(N) running sums instead of O(N×K) window iteration
  - RSI/MACD eliminate 3 Vec allocations in hot paths
  - CVaR computes tail mean on iterator (no intermediate Vec)
- **Code quality**: Extracted helper functions to reduce duplication
  - Binance client: `check_response()`, `validate_query_params()`
  - Risk checks: `cmp_symbol()`, `ratio_or_inf()`, `exposure()`
  - Indicators: `rsi_from_avgs()` (de-duplicated seed + loop logic)
  - Metrics: `rolling_window()` shared by rolling Sharpe/volatility

### Fixed

- **Security (audit findings)**:
  - Validated Binance query params to prevent URL parameter injection
  - Safe `u64→i64` casts in risk checks with `try_from()` + `saturating_mul()`
  - Used `saturating_abs()` to fix negative price bypass and `i64::MIN` panic
  - Fail all risk checks when equity ≤ 0 (was silently passing, incorrect)
  - Guard `CostModel` `u128→i64` cast with `try_from()`
  - Zeroize Binance API keys on drop (prevents leak in debug/logs)
  - Redact order params from debug logs (prevent sensitive data leak)
- **Correctness**:
  - CV splits now match sklearn: `test_starts = range(n - k*test_size, n, test_size)`
  - MACD: align fast EMA start with slow EMA for correct initialization
  - CVaR: use parametric VaR (`norm.ppf`) matching quantstats convention
  - Spearman p-value: custom incomplete beta via Newton-Raphson `betacf` + symmetry
- **Overflow safety**: Portfolio `execute_fill()` uses `saturating_abs/mul/sub`
- **Clippy**: Fixed `iter_cloned_collect`, `needless_range_loop`, `excessive_precision`, `inconsistent_digit_grouping`

### Removed

- **ta-lib dependency**: All indicators reimplemented in pure Rust (breaking change if using C library directly)

## [0.7.0] - 2026-02-09

### Added

- **`nanobook-broker` crate**: Generic `Broker` trait with IBKR and Binance implementations
  - `MockBroker` with builder pattern, configurable fill modes, order recording
  - IBKR: TWS/Gateway blocking client, order execution with fill monitoring
  - Binance: REST spot client, HMAC-SHA256 auth, book ticker quotes
- **`nanobook-risk` crate**: Pre-trade risk engine
  - `RiskEngine::check_order()` — single-order position/leverage/short checks
  - `RiskEngine::check_batch()` — batch validation with aggregate limits
  - `RiskConfig::validate()` — fail-fast config validation at construction
- **Backtest bridge** (`backtest_weights`): Schedule-driven portfolio simulator
  with input validation (NaN/Inf, mismatched lengths, negative prices)
- **`Symbol::from_str_truncated()`**: Safe truncation with UTF-8 boundary handling
  for external input (broker feeds, ITCH data)
- **CI hardening**:
  - `cargo-deny` + `cargo-audit` security scanning with `deny.toml` policy
  - MIRI for undefined behavior detection (strict provenance, alignment checks)
  - `cargo-llvm-cov` code coverage → Codecov
- **446 tests** (was ~333, +34%):
  - Property tests: backtest bridge, portfolio overflow, risk engine
  - Edge cases: adversarial inputs for all public APIs
  - Risk engine `check_order` tests (was zero)
  - Broker parsing: Binance JSON round-trips, IBKR type tests
  - Rebalancer integration: execution helpers, constraint overrides, diff

### Changed

- `#[track_caller]` on `Symbol::new()` for better panic diagnostics
- Bare `unwrap()` → `expect("invariant: ...")` in matching engine and stop book
- Portfolio `unwrap()` sites → graceful `match` patterns
- Rebalancer execution helpers promoted to `pub` for testability
- `RiskConfig` gains `Default` impl (reuses serde defaults)

### Fixed

- Binance auth clock panic: `.expect()` → `.unwrap_or(Duration::ZERO)`
- Backtest bridge `.zip()` silently truncating mismatched schedule lengths

### Removed

- `examples/demo.rs` — 354-line educational walkthrough (superseded by `basic_usage.rs`)
- `SPECS.md` — outdated technical spec (superseded by `DOC.md`)

## [0.6.0] - 2026-02-06

### Added

- **O(1) order cancellation**: Tombstone-based cancellation in `Level` and `OrderBook`
  - ~350x speedup for deep level cancels (170 ns vs ~60 μs)
  - `Exchange::compact()` — manual compaction to reclaim tombstone memory
- **NASDAQ ITCH 5.0 parser** (feature: `itch`):
  - `ItchParser` — streaming binary parser for ITCH 5.0 protocol
  - Handles Add, Replace, Execute, Delete, Trade, and StockDirectory messages
  - `parse_itch()` exposed to Python
- **Expanded benchmarks**: Modify, event apply, multi-symbol throughput
  - Dedicated `stops.rs` benchmark for trigger cascades and trailing updates
  - CI regression detection against v0.5 baseline

### Changed

- `sweep_equal_weight` renamed to cleaner API name
- Python type stubs updated for new methods

## [0.5.0] - 2026-02-06

### Added

- **Complete Python bindings** (`pip install nanobook` via maturin):
  - `Order`, `Position`, `Event` classes
  - `Exchange`: `events()`, `replay()`, `full_book()`, stop order queries
  - `Portfolio`: position tracking, LOB rebalancing, snapshots
  - `MultiExchange`: method forwarding, `best_prices()`
  - `Strategy`: custom Python callback support in `run_backtest()`
- **Type stubs** (`nanobook.pyi`) for IDE support
- **Automated wheel builds** for Linux, macOS, Windows in CI
- 80 Python tests

### Changed

- Modernized to Rust 2024 edition (MSRV 1.85)
- Requires Python >= 3.11

## [0.4.0] - 2026-02-06

### Added

- **Trailing stops**: Multi-method trailing stop orders
  - `submit_trailing_stop_market()` — trailing stop with market trigger
  - `submit_trailing_stop_limit()` — trailing stop with limit trigger
  - `TrailMethod::Fixed(offset)` — fixed-offset trailing
  - `TrailMethod::Percentage(pct)` — percentage-based trailing
  - `TrailMethod::Atr { multiplier, period }` — ATR-based adaptive trailing
  - Watermark tracking: sell trailing tracks highs, buy trailing tracks lows
  - Stop price re-indexes automatically when watermark updates
  - Internal ATR computation from tick-level price changes
- **Strategy trait** (feature: `portfolio`):
  - `Strategy` trait — `compute_weights(bar_index, prices, portfolio) -> Vec<(Symbol, f64)>`
  - `run_backtest()` — orchestrates rebalance-record loop
  - `EqualWeight` — built-in equal-weight strategy implementation
  - `BacktestResult` — portfolio + optional metrics
  - `sweep_strategy()` — parallel parameter sweep over strategy instances
- **Portfolio persistence** (feature: `persistence`):
  - `Portfolio::save_json()` / `Portfolio::load_json()` — JSON serialization
  - `FxHashMap<Symbol, Position>` serde via ordered vec conversion
  - `Metrics` serde support
- **Python bindings** (`pip install nanobook` via maturin):
  - `nanobook.Exchange` — full exchange API with string-based enums
  - `nanobook.Portfolio` — portfolio management and rebalancing
  - `nanobook.CostModel` — transaction cost modeling
  - `nanobook.py_compute_metrics()` — financial metrics from return series
  - `nanobook.py_sweep_equal_weight()` — parallel sweep with GIL release
  - Stop orders, trailing stops, and all query methods
  - 39 Python tests covering exchange, portfolio, and sweep
- **Portfolio benchmarks**: Criterion benchmarks for backtest and sweep performance

### Changed

- `CostModel` now derives `Copy` (was `Clone` only)
- `Event` enum no longer derives `Eq` (only `PartialEq`) due to `f64` in `TrailMethod`
- Workspace layout: `python/` added as workspace member

## [0.3.0] - 2026-02-06

### Added

- **Symbol type**: Fixed-size `Symbol([u8; 8], u8)` — `Copy`, no heap allocation, max 8 ASCII bytes
  - `Symbol::new()`, `try_new()`, `Display`, `Debug`, `AsRef<str>`
  - Custom serde support (serializes as string)
- **MultiExchange**: Multi-symbol LOB — one `Exchange` per `Symbol`
  - `get_or_create(symbol)`, `get(symbol)`, `best_prices()`, `symbols()`
- **Portfolio engine** (feature: `portfolio`):
  - `Portfolio` — cash + positions + cost model + equity tracking
  - `Position` — per-symbol tracking with VWAP entry, realized/unrealized PnL
  - `CostModel` — commission + slippage in basis points, minimum fee
  - `rebalance_simple()` — instant execution for fast parameter sweeps
  - `rebalance_lob()` — route through real LOB matching engines
  - `record_return()`, `snapshot()`, `current_weights()`, `equity_curve()`
- **Financial metrics** (feature: `portfolio`):
  - `compute_metrics()` — Sharpe, Sortino, CAGR, max drawdown, Calmar, volatility
  - `Metrics` struct with `Display` for formatted output
- **Parallel sweep** (feature: `parallel`):
  - `sweep()` — rayon-based parallel parameter sweep over strategy configurations
- **Book analytics**:
  - `BookSnapshot::imbalance()` — order book imbalance ratio
  - `BookSnapshot::weighted_mid()` — volume-weighted midpoint price
  - `Trade::vwap()` — volume-weighted average price across trades
- **Examples**: `portfolio_backtest`, `multi_symbol_lob`
- **Tests**: `portfolio_invariants` integration test suite

### Changed

- `Symbol` added to core types (not feature-gated)
- `MultiExchange` added to public API (not feature-gated)

## [0.2.0] - 2026-02-05

### Added

- **Stop orders**: Stop-market and stop-limit orders with automatic triggering
  - `submit_stop_market()` — triggers market order on price threshold
  - `submit_stop_limit()` — triggers limit order on price threshold
  - Cascading triggers with depth limit (max 100 iterations)
  - `cancel()` works on both regular and stop orders
  - New types: `StopOrder`, `StopStatus`, `StopBook`, `StopSubmitResult`
- **Input validation**: `try_submit_limit()` and `try_submit_market()` with `ValidationError`
  - `ZeroQuantity` — quantity must be > 0
  - `ZeroPrice` — price must be > 0 for limit orders
- **Serde support**: Optional `serde` feature flag adds `Serialize`/`Deserialize` to all public types
- **Persistence**: Optional `persistence` feature for file-based event sourcing
  - `exchange.save(path)` / `Exchange::load(path)` — JSON Lines format
  - `save_events()` / `load_events()` — lower-level API
- **Examples**: `basic_usage`, `market_making`, `ioc_execution`
- **CLI commands**: `stop`, `stoplimit`, `save`, `load`

### Changed

- `cancel()` now checks stop book before regular order book
- `clear_order_history()` also clears triggered/cancelled stop orders
- Event enum extended with `SubmitStopMarket` and `SubmitStopLimit` variants

## [0.1.0] - 2026-02-05

Initial release of nanobook - a deterministic limit order book and matching engine.

### Added

- **Core types**: `Price`, `Quantity`, `Timestamp`, `OrderId`, `TradeId`, `Side`
- **Order management**: Limit orders, market orders, cancel, and modify operations
- **Time-in-force**: GTC (good-til-cancelled), IOC (immediate-or-cancel), FOK (fill-or-kill)
- **Matching engine**: Price-time priority with partial fills and price improvement
- **Event logging**: Optional replay capability via feature flag (`event-log`)
- **Snapshots**: L2 order book depth snapshots
- **CLI binary**: Interactive `lob` command for exploration
- **Examples**: `demo` (interactive) and `demo_quick` (non-interactive)
- **Benchmarks**: Criterion-based throughput and latency measurements
- **CI/CD**: GitHub Actions for testing (Ubuntu/macOS), linting, and releases
- **Multi-platform releases**: Linux (x86_64, aarch64), macOS (Intel, Silicon), Windows

### Performance

- 8.3M orders/sec submission throughput (no match)
- 5M orders/sec with matching
- Sub-microsecond latencies (120ns submit, 1ns BBO query)
- O(1) best bid/ask queries via caching
- FxHash for fast order lookups

### Technical

- Rust 2021 edition, MSRV 1.70 (upgraded to Rust 2024 / MSRV 1.85 in v0.5.0)
- Minimal dependencies: `thiserror`, `rustc-hash`
- Fixed-point price representation (avoids floating-point errors)
- Deterministic via monotonic timestamps (not system clock)

[Unreleased]: https://github.com/ricardofrantz/nanobook/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/ricardofrantz/nanobook/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/ricardofrantz/nanobook/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/ricardofrantz/nanobook/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/ricardofrantz/nanobook/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ricardofrantz/nanobook/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ricardofrantz/nanobook/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ricardofrantz/nanobook/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ricardofrantz/nanobook/releases/tag/v0.1.0
