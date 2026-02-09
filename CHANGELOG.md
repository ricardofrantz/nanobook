# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/),
and this project adheres to [Semantic Versioning](https://semver.org/).

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
- **Extended portfolio metrics**: `cvar`, `win_rate`, `profit_factor`, `payoff_ratio`, `kelly_criterion`, `rolling_sharpe()`, `rolling_volatility()`
- **Python bindings**: All new functions exposed via PyO3 with NumPy integration
- **Property tests**: Hypothesis-based tests for indicators, stats, CV (44 new tests)
- **Reference tests**: Validation against ta-lib, scipy, sklearn

### Changed

- Rolling metrics use O(N) running sums instead of O(N×K) window iteration
- RSI/MACD eliminate 3 Vec allocations in hot paths
- Extracted helper functions to reduce duplication across binance client, risk checks, indicators, and metrics

### Fixed

- Validated Binance query params to prevent URL parameter injection
- Safe `u64→i64` casts in risk checks with `try_from()` + `saturating_mul()`
- Used `saturating_abs()` to fix negative price bypass and `i64::MIN` panic
- Fail all risk checks when equity ≤ 0 (was silently passing)
- CV splits now match sklearn
- MACD: align fast EMA start with slow EMA for correct initialization
- Portfolio `execute_fill()` uses `saturating_abs/mul/sub` for overflow safety

### Removed

- ta-lib dependency: all indicators reimplemented in pure Rust

## [0.7.0] - 2026-02-09

### Added

- `nanobook-rebalancer` crate: IBKR portfolio execution bridge

### Changed

- Upgraded pyo3 0.23 → 0.24 (fix RUSTSEC-2025-0020)
- Hardened ITCH parser: validate message length before indexing
- Revised CI and release workflows
- Updated cargo-deny config for 0.19 format

### Fixed

- Clippy (Rust 1.93), bench compile, Python venv issues in CI

## [0.6.0] - 2026-02-06

### Added

- **O(1) order cancellation**: Tombstone-based cancellation (~350x speedup)
- `Exchange::compact()` — manual compaction to reclaim tombstone memory
- **NASDAQ ITCH 5.0 parser** (feature: `itch`): streaming binary parser
- `parse_itch()` exposed to Python
- Expanded benchmarks: modify, event apply, multi-symbol throughput, stop trigger cascades

### Changed

- `sweep_equal_weight` renamed to cleaner API name
- Python type stubs updated for new methods

## [0.5.0] - 2026-02-06

### Added

- **Complete Python bindings** via maturin: `Order`, `Position`, `Event`, `Exchange`, `Portfolio`, `MultiExchange`, `Strategy`
- Type stubs (`nanobook.pyi`) for IDE support
- Automated wheel builds for Linux, macOS, Windows in CI
- 80 Python tests

### Changed

- Modernized to Rust 2024 edition (MSRV 1.85)
- Requires Python >= 3.11

## [0.4.0] - 2026-02-06

### Added

- Trailing stop orders with configurable trail distance
- Strategy trait for backtesting with custom callbacks
- Initial Python bindings via PyO3

## [0.3.0] - 2026-02-06

### Added

- Portfolio engine with position tracking and P&L
- Multi-symbol exchange support
- Analytics: Sharpe ratio, max drawdown, annualized returns
- Parallel parameter sweep for strategy optimization

## [0.2.0] - 2026-02-05

### Added

- Stop orders (stop-limit, stop-market) with trigger logic
- Order validation and error types
- Serde serialization for all core types
- Exchange state persistence (save/restore)

### Fixed

- Clippy digit grouping convention in tests

## [0.1.0] - 2026-02-05

### Added

- Limit order book with price-time priority
- `Order`, `Level`, `OrderBook`, `Exchange` core types
- Market, limit, and IOC order types
- Event-driven matching engine
- Initial CI workflow

[Unreleased]: https://github.com/ricardofrantz/nanobook/compare/v0.8.0...HEAD
[0.8.0]: https://github.com/ricardofrantz/nanobook/compare/v0.7.0...v0.8.0
[0.7.0]: https://github.com/ricardofrantz/nanobook/compare/v0.6.0...v0.7.0
[0.6.0]: https://github.com/ricardofrantz/nanobook/compare/v0.5.0...v0.6.0
[0.5.0]: https://github.com/ricardofrantz/nanobook/compare/v0.4.0...v0.5.0
[0.4.0]: https://github.com/ricardofrantz/nanobook/compare/v0.3.0...v0.4.0
[0.3.0]: https://github.com/ricardofrantz/nanobook/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/ricardofrantz/nanobook/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ricardofrantz/nanobook/commits/v0.1.0
