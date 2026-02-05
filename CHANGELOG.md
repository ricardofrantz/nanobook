# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

- Rust 2021 edition, MSRV 1.70
- Minimal dependencies: `thiserror`, `rustc-hash`
- Fixed-point price representation (avoids floating-point errors)
- Deterministic via monotonic timestamps (not system clock)

[Unreleased]: https://github.com/ricardofrantz/nanobook/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/ricardofrantz/nanobook/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/ricardofrantz/nanobook/releases/tag/v0.1.0
