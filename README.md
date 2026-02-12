# nanobook

[![CI](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml/badge.svg)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/nanobook.svg)](https://crates.io/crates/nanobook)
[![docs.rs](https://docs.rs/nanobook/badge.svg)](https://docs.rs/nanobook)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![MIRI](https://img.shields.io/badge/MIRI-clean-brightgreen)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![cargo-deny](https://img.shields.io/badge/cargo--deny-audited-brightgreen)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)

**Production-grade Rust execution infrastructure for automated trading.**
Zero-allocation hot paths. No panics on external input. MIRI-verified memory safety.
Python computes the strategy. nanobook handles everything else.

## Architecture

```
┌─────────────────────────────────────────────────┐
│        Your Python Strategy  (private)          │
│   Factors · Signals · Sizing · Scheduling       │
├─────────────────────────────────────────────────┤
│            nanobook  (Rust, open-source)         │
│  ┌──────────┬──────────┬──────────┬──────────┐  │
│  │  Broker  │   Risk   │Portfolio │   LOB    │  │
│  │   IBKR   │  Engine  │Simulator │  Engine  │  │
│  │  Binance │ PreTrade │ Backtest │ 8M ops/s │  │
│  └──────────┴──────────┴──────────┴──────────┘  │
│   Rebalancer CLI: weights → diff → execute      │
└─────────────────────────────────────────────────┘
```

Python computes **what** to trade — factor rankings, signals, target weights.
nanobook executes **how** — order routing, risk checks, portfolio simulation,
and a deterministic matching engine. Clean separation: strategy logic stays
in Python, execution runs in Rust.

## Workspace

| Crate | Description |
|-------|-------------|
| `nanobook` | LOB matching engine, portfolio simulator, backtest bridge, GARCH, optimizers |
| `nanobook-broker` | Broker trait with IBKR and Binance adapters |
| `nanobook-risk` | Pre-trade risk engine (position limits, leverage, short exposure) |
| `nanobook-python` | PyO3 bindings for all layers |
| `nanobook-rebalancer` | CLI: target weights → IBKR execution with audit trail |

## Install

**Python:**

```bash
pip install nanobook
```

**Rust:**

```toml
[dependencies]
nanobook = "0.9"
```

**From source:**

```bash
git clone https://github.com/ricardofrantz/nanobook
cd nanobook
cargo build --release
cargo test

# Python bindings
cd python && maturin develop --release

# Binance adapter (feature-gated, not in PyPI wheels)
cd python && maturin develop --release --features binance
```

## The Bridge: Python Strategy → Rust Execution

The canonical integration pattern — Python computes a weight schedule,
Rust simulates the portfolio and returns metrics:

```python
import nanobook

result = nanobook.backtest_weights(
    weight_schedule=[
        [("AAPL", 0.5), ("MSFT", 0.5)],
        [("AAPL", 0.3), ("NVDA", 0.7)],
    ],
    price_schedule=[
        [("AAPL", 185_00), ("MSFT", 370_00)],
        [("AAPL", 190_00), ("MSFT", 380_00), ("NVDA", 600_00)],
    ],
    initial_cash=1_000_000_00,  # $1M in cents
    cost_bps=15,                # 15 bps round-trip
    stop_cfg={"trailing_stop_pct": 0.05},
)

print(f"Sharpe: {result['metrics'].sharpe:.2f}")
print(f"Max DD: {result['metrics'].max_drawdown:.1%}")
print(result["holdings"][-1])    # per-period symbol weights
print(result["stop_events"])     # stop trigger metadata
```

Your optimizer produces weights. `backtest_weights()` handles rebalancing,
cost modeling, position tracking, and return computation at compiled speed
with the GIL released.

**v0.9 additions:** GARCH(1,1) forecasting, portfolio optimizers
(min-variance, max-Sharpe, risk-parity, CVaR, CDaR), and trailing/fixed
stop-loss simulation — all accessible from Python.

### Optimizer Example

```python
import nanobook
import numpy as np

# Daily returns matrix (T × N)
returns = np.random.randn(252, 5) * 0.01

weights = nanobook.optimize_max_sharpe(returns, risk_free_rate=0.0)
print(dict(zip(["A","B","C","D","E"], weights)))
```

## Layer Examples

### LOB Engine (Rust)

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(50_00), 100, TimeInForce::GTC);
let result = exchange.submit_limit(Side::Buy, Price(50_00), 100, TimeInForce::GTC);

assert_eq!(result.trades.len(), 1);
assert_eq!(result.trades[0].price, Price(50_00));
```

### Portfolio + Metrics (Python)

```python
portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel(commission_bps=5))
portfolio.rebalance_simple([("AAPL", 0.6)], [("AAPL", 150_00)])
portfolio.record_return([("AAPL", 155_00)])
metrics = portfolio.compute_metrics(252.0, 0.0)
print(f"Sharpe: {metrics.sharpe:.2f}")
```

### Broker + Risk (Python)

```python
# Pre-trade risk check
risk = nanobook.RiskEngine(max_position_pct=0.25, max_leverage=1.5)
checks = risk.check_order("AAPL", "buy", 100, 185_00,
                          equity_cents=1_000_000_00,
                          positions=[("AAPL", 200)])

# Execute through IBKR
broker = nanobook.IbkrBroker("127.0.0.1", 4002, client_id=1)
broker.connect()
oid = broker.submit_order("AAPL", "buy", 100, order_type="limit",
                          limit_price_cents=185_00)
```

### Rebalancer CLI

```bash
# Build
cargo build -p nanobook-rebalancer --release

# Dry run — show plan without executing
rebalancer run target.json --dry-run

# Execute with confirmation prompt
rebalancer run target.json

# Compare actual vs target positions
rebalancer reconcile target.json
```

## Performance

Single-threaded benchmarks (AMD Ryzen / Intel Core):

| Operation | Latency | Throughput |
|-----------|---------|------------|
| Submit (no match) | 120 ns | 8.3M ops/sec |
| Submit (with match) | ~200 ns | 5M ops/sec |
| BBO query | ~1 ns | 1B ops/sec |
| Cancel (tombstone) | 170 ns | 5.9M ops/sec |
| L2 snapshot (10 levels) | ~500 ns | 2M ops/sec |

Single-threaded throughput is roughly equivalent to Numba (both compile to
LLVM IR). Where Rust wins: zero cold-start, true parallelism via Rayon with
no GIL contention, and deterministic memory without GC pauses.

```bash
cargo bench
```

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `event-log` | Yes | Event recording for deterministic replay |
| `serde` | No | Serialize/deserialize all public types |
| `persistence` | No | File-based event sourcing (JSON Lines) |
| `portfolio` | No | Portfolio engine, position tracking, metrics, strategy trait |
| `parallel` | No | Rayon-based parallel parameter sweeps |
| `itch` | No | NASDAQ ITCH 5.0 binary protocol parser |

## Design Constraints

Engineering decisions that keep the system simple and fast:

- **Single-threaded** — deterministic by design; same inputs always produce same outputs
- **In-process** — no networking overhead; wrap externally if needed
- **No compliance layer** — no self-trade prevention or circuit breakers (out of scope)
- **No complex order types** — no iceberg or pegged orders

## Documentation

- Full developer reference is merged below in this README (`## Full Reference (Merged from DOC.md)`).
- **[docs.rs](https://docs.rs/nanobook)** — Rust API docs

## License

MIT

## Full Reference (Merged from DOC.md)


[![CI](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml/badge.svg)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/nanobook.svg)](https://crates.io/crates/nanobook)
[![docs.rs](https://docs.rs/nanobook/badge.svg)](https://docs.rs/nanobook)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Developer Reference** — Full API documentation for the nanobook workspace.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
- [Exchange API](#exchange-api)
- [Types Reference](#types-reference)
- [Book Snapshots](#book-snapshots)
- [Stop Orders & Trailing Stops](#stop-orders--trailing-stops)
- [Event Replay](#event-replay)
- [Symbol & MultiExchange](#symbol--multiexchange)
- [Portfolio Engine](#portfolio-engine)
- [Strategy Trait](#strategy-trait)
- [Backtest Bridge](#backtest-bridge)
- [Broker Abstraction](#broker-abstraction)
- [Risk Engine](#risk-engine)
- [Rebalancer CLI](#rebalancer-cli)
- [Python Bindings](#python-bindings)
- [Book Analytics](#book-analytics)
- [Persistence & Serde](#persistence--serde)
- [CLI Reference](#cli-reference)
- [Performance](#performance)
- [Comparison with Other Rust LOBs](#comparison-with-other-rust-lobs)
- [Design Constraints](#design-constraints)

---

## Quick Start

```toml
[dependencies]
nanobook = "0.9"
```

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(50_00), 100, TimeInForce::GTC);
let result = exchange.submit_limit(Side::Buy, Price(50_00), 100, TimeInForce::GTC);

assert_eq!(result.trades.len(), 1);
assert_eq!(result.trades[0].price, Price(50_00));
```

---

## Core Concepts

### Prices

Prices are integers in the **smallest currency unit** (cents for USD), avoiding floating-point errors.

```rust
let price = Price(100_50);  // $100.50
assert!(Price(100_00) < Price(101_00));
```

Display formats as dollars: `Price(10050)` prints as `$100.50`.

Constants: `Price::ZERO`, `Price::MAX` (market buys), `Price::MIN` (market sells).

### Quantities and Timestamps

- `Quantity = u64` — shares or contracts, always positive.
- `Timestamp = u64` — monotonic nanosecond counter (not system clock), guaranteeing deterministic ordering.

### Determinism

No randomness anywhere. Same sequence of operations always produces identical trades. Event replay reconstructs exact state.

---

## Exchange API

`Exchange` is the main entry point, wrapping an `OrderBook` with order submission, cancellation, modification, and queries.

### Order Submission

```rust
// Limit order — matches against opposite side, remainder handled by TIF
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

// Market order — IOC semantics at Price::MAX (buy) or Price::MIN (sell)
let result = exchange.submit_market(Side::Buy, 500);
```

### Order Management

```rust
// Cancel — O(1) via tombstones
let result = exchange.cancel(order_id);  // CancelResult { success, cancelled_quantity, error }

// Modify — cancel + replace (loses time priority, gets new OrderId)
let result = exchange.modify(order_id, Price(101_00), 200);
```

### Queries

```rust
let (bid, ask) = exchange.best_bid_ask();  // L1 — O(1)
let spread = exchange.spread();             // Option<i64>
let snap = exchange.depth(10);              // L2 — top 10 levels
let full = exchange.full_book();            // L3 — everything
let order = exchange.get_order(OrderId(1)); // Option<&Order>
let trades = exchange.trades();             // &[Trade]
```

### Memory Management

```rust
exchange.clear_trades();           // Clear trade history
exchange.clear_order_history();    // Remove filled/cancelled orders
exchange.compact();                // Reclaim tombstone memory
```

### Input Validation

The `try_submit_*` methods validate inputs before processing:

```rust
let err = exchange.try_submit_limit(Side::Buy, Price(0), 100, TimeInForce::GTC);
assert_eq!(err.unwrap_err(), ValidationError::ZeroPrice);
```

### Time-in-Force Semantics

| TIF | Partial Fill? | Rests on Book? | No Liquidity |
|-----|:---:|:---:|---|
| **GTC** | Yes | Yes (remainder) | Rests entirely |
| **IOC** | Yes | No (remainder cancelled) | Cancelled |
| **FOK** | No | No (all-or-nothing) | Cancelled |

---

## Types Reference

| Type | Definition | Description | Display |
|------|-----------|-------------|---------|
| `Price` | `struct Price(pub i64)` | Price in smallest units (cents) | `$100.50` |
| `Quantity` | `type Quantity = u64` | Number of shares/contracts | — |
| `OrderId` | `struct OrderId(pub u64)` | Unique order identifier | `O42` |
| `TradeId` | `struct TradeId(pub u64)` | Unique trade identifier | `T7` |
| `Timestamp` | `type Timestamp = u64` | Nanosecond counter (not wall clock) | — |

### Result Types

```rust
pub struct SubmitResult {
    pub order_id: OrderId,
    pub status: OrderStatus,          // New | PartiallyFilled | Filled | Cancelled
    pub trades: Vec<Trade>,
    pub filled_quantity: Quantity,
    pub resting_quantity: Quantity,
    pub cancelled_quantity: Quantity,
}

pub struct Trade {
    pub id: TradeId,
    pub price: Price,                 // Resting order's price (aggressor gets price improvement)
    pub quantity: Quantity,
    pub aggressor_order_id: OrderId,
    pub passive_order_id: OrderId,
    pub aggressor_side: Side,
    pub timestamp: Timestamp,
}
```

### Enums

| Enum | Variants | Key Methods |
|------|----------|-------------|
| `Side` | `Buy`, `Sell` | `opposite()` |
| `TimeInForce` | `GTC`, `IOC`, `FOK` | `can_rest()`, `allows_partial()` |
| `OrderStatus` | `New`, `PartiallyFilled`, `Filled`, `Cancelled` | `is_active()`, `is_terminal()` |

---

## Book Snapshots

```rust
pub struct BookSnapshot {
    pub bids: Vec<LevelSnapshot>,  // Highest price first
    pub asks: Vec<LevelSnapshot>,  // Lowest price first
    pub timestamp: Timestamp,
}

pub struct LevelSnapshot {
    pub price: Price,
    pub quantity: Quantity,
    pub order_count: usize,
}
```

| Method | Returns |
|--------|---------|
| `snap.best_bid()` / `best_ask()` | `Option<Price>` |
| `snap.spread()` | `Option<i64>` |
| `snap.mid_price()` | `Option<f64>` |
| `snap.total_bid_quantity()` / `total_ask_quantity()` | `Quantity` |
| `snap.imbalance()` | `Option<f64>` — [-1.0, 1.0], positive = buy pressure |
| `snap.weighted_mid()` | `Option<f64>` — leans toward less liquid side |

---

## Stop Orders & Trailing Stops

### Stop Orders

```rust
// Stop-market: triggers market order when last trade price hits stop
exchange.submit_stop_market(Side::Sell, Price(95_00), 100);

// Stop-limit: triggers limit order at limit_price when stop hits
exchange.submit_stop_limit(Side::Sell, Price(95_00), Price(94_50), 100, TimeInForce::GTC);
```

| Side | Triggers When |
|------|---------------|
| Buy stop | `last_trade_price >= stop_price` |
| Sell stop | `last_trade_price <= stop_price` |

Key behaviors: immediate trigger if price already past stop, cascade up to 100 iterations, cancel via `exchange.cancel(stop_id)`.

### Trailing Stops

Three trailing methods — stop price tracks the market and only moves in the favorable direction:

```rust
// Fixed: triggers if price drops $2.00 from peak
exchange.submit_trailing_stop_market(Side::Sell, Price(98_00), 100, TrailMethod::Fixed(200));

// Percentage: trail by 5% from peak
exchange.submit_trailing_stop_market(Side::Sell, Price(95_00), 100, TrailMethod::Percentage(0.05));

// ATR-based: adaptive trailing using 2x ATR over 14-period window
exchange.submit_trailing_stop_market(Side::Sell, Price(95_00), 100,
    TrailMethod::Atr { multiplier: 2.0, period: 14 });
```

Trailing stop-limit variant: `submit_trailing_stop_limit()` — same parameters plus `limit_price` and `TimeInForce`.

---

## Event Replay

**Feature flag:** `event-log` (enabled by default)

Every operation is recorded as an `Event`. Replaying events on a fresh exchange produces identical state.

```rust
// Save events
let events = exchange.events().to_vec();

// Reconstruct exact state
let replayed = Exchange::replay(&events);
assert_eq!(exchange.best_bid_ask(), replayed.best_bid_ask());
```

Event types: `SubmitLimit`, `SubmitMarket`, `Cancel`, `Modify`.

Disable for max performance:

```toml
nanobook = { version = "0.6", default-features = false }
```

---

## Symbol & MultiExchange

### Symbol

Fixed-size instrument identifier. `[u8; 8]` inline — `Copy`, no heap allocation, max 8 ASCII bytes.

```rust
let sym = Symbol::new("AAPL");
assert!(Symbol::try_new("TOOLONGNAME").is_none());
```

### MultiExchange

Independent per-symbol order books:

```rust
let mut multi = MultiExchange::new();
let aapl = Symbol::new("AAPL");

multi.get_or_create(&aapl).submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);

for (sym, bid, ask) in multi.best_prices() {
    println!("{sym}: bid={bid:?} ask={ask:?}");
}
```

---

## Portfolio Engine

**Feature flag:** `portfolio`

Tracks cash, positions, costs, returns, and equity over time.

```rust
use nanobook::portfolio::{Portfolio, CostModel};

let cost = CostModel { commission_bps: 5, slippage_bps: 3, min_trade_fee: 1_00 };
let mut portfolio = Portfolio::new(1_000_000_00, cost);

// Rebalance to target weights
portfolio.rebalance_simple(&[(Symbol::new("AAPL"), 0.6)], &[(Symbol::new("AAPL"), 150_00)]);

// Record period return and compute metrics
portfolio.record_return(&[(Symbol::new("AAPL"), 155_00)]);
let metrics = compute_metrics(portfolio.returns(), 252.0, 0.0);
```

### Execution Modes

- **SimpleFill** — instant at bar prices: `portfolio.rebalance_simple(targets, prices)`
- **LOBFill** — route through `Exchange` matching engines: `portfolio.rebalance_lob(targets, exchanges)`

### Position

Per-symbol tracking with VWAP entry price and realized PnL:

```rust
let mut pos = Position::new(Symbol::new("AAPL"));
pos.apply_fill(100, 150_00);   // buy 100 @ $150
pos.apply_fill(-50, 160_00);   // sell 50 @ $160 → $500 realized PnL
```

### Financial Metrics

`compute_metrics(&returns, periods_per_year, risk_free)` returns: `total_return`, `cagr`, `volatility`, `sharpe`, `sortino`, `max_drawdown`, `calmar`, `num_periods`, `winning_periods`, `losing_periods`.

### Parallel Sweep

**Feature flag:** `parallel` (implies `portfolio`)

```rust
use nanobook::portfolio::sweep::sweep;

let results = sweep(&params, 12.0, 0.0, |&leverage| {
    vec![0.01 * leverage, -0.005 * leverage]
});
```

---

## Strategy Trait

**Feature flag:** `portfolio`

Implement `compute_weights()` for batch-oriented backtesting:

```rust
impl Strategy for MomentumStrategy {
    fn compute_weights(
        &self,
        bar_index: usize,
        prices: &[(Symbol, i64)],
        _portfolio: &Portfolio,
    ) -> Vec<(Symbol, f64)> {
        if bar_index < self.lookback { return vec![]; }
        let w = 1.0 / prices.len() as f64;
        prices.iter().map(|(sym, _)| (*sym, w)).collect()
    }
}

let result = run_backtest(&strategy, &price_series, 1_000_000_00, CostModel::zero(), 12.0, 0.0);
```

Built-in: `EqualWeight` strategy. Parallel variant: `sweep_strategy()`.

---

## Backtest Bridge

The bridge between Python strategy code and Rust execution. Python computes a weight schedule,
Rust simulates the portfolio at compiled speed.

### Rust API

```rust
use nanobook::backtest_bridge::backtest_weights;

let result = backtest_weights(
    &weight_schedule,    // &[Vec<(Symbol, f64)>] — target weights per period
    &price_schedule,     // &[Vec<(Symbol, i64)>] — prices per period
    1_000_000_00,        // initial cash in cents
    15,                  // cost in basis points
    252.0,               // periods per year
    0.0,                 // risk-free rate per period
);
```

Returns `BacktestBridgeResult`:

| Field | Type | Description |
|-------|------|-------------|
| `returns` | `Vec<f64>` | Per-period returns |
| `equity_curve` | `Vec<i64>` | Equity at each period (cents) |
| `final_cash` | `i64` | Ending cash balance |
| `metrics` | `Option<Metrics>` | Sharpe, Sortino, max drawdown, etc. |
| `holdings` | `Vec<Vec<(Symbol, f64)>>` | Per-period holdings weights |
| `symbol_returns` | `Vec<Vec<(Symbol, f64)>>` | Per-period close-to-close symbol returns |
| `stop_events` | `Vec<BacktestStopEvent>` | Stop trigger metadata (index, symbol, price, reason) |

### Python API

```python
result = nanobook.py_backtest_weights(
    weight_schedule=[[("AAPL", 0.5), ("MSFT", 0.5)], ...],
    price_schedule=[[("AAPL", 185_00), ("MSFT", 370_00)], ...],
    initial_cash=1_000_000_00,
    cost_bps=15,
    periods_per_year=252.0,
    risk_free=0.0,
    stop_cfg={"trailing_stop_pct": 0.05},
)
# result["returns"], result["equity_curve"], result["metrics"],
# result["holdings"], result["symbol_returns"], result["stop_events"]
```

GIL is released during computation for maximum throughput.

Clean aliases (no `py_` prefix) are exported for new integrations:
`backtest_weights`, `capabilities`, `garch_forecast`, and `optimize_*`.

### qtrade v0.4 Bridge Pattern

Capability probing contract used by `calc.bridge`:

```python
import nanobook

def has_nanobook_feature(name: str) -> bool:
    caps = set(nanobook.py_capabilities()) if hasattr(nanobook, "py_capabilities") else set()
    if name in caps:
        return True

    symbol_map = {
        "backtest_stops": "py_backtest_weights",
        "garch_forecast": "py_garch_forecast",
        "optimize_min_variance": "py_optimize_min_variance",
        "optimize_max_sharpe": "py_optimize_max_sharpe",
        "optimize_risk_parity": "py_optimize_risk_parity",
        "optimize_cvar": "py_optimize_cvar",
        "optimize_cdar": "py_optimize_cdar",
        "backtest_holdings": "py_backtest_weights",
    }
    sym = symbol_map.get(name)
    return bool(sym and hasattr(nanobook, sym))
```

---

## Broker Abstraction

**Crate:** `nanobook-broker`

Generic trait over brokerages with concrete adapters for IBKR and Binance.

### Broker Trait

```rust
pub trait Broker {
    fn connect(&mut self) -> Result<(), BrokerError>;
    fn disconnect(&mut self) -> Result<(), BrokerError>;
    fn positions(&self) -> Result<Vec<Position>, BrokerError>;
    fn account(&self) -> Result<Account, BrokerError>;
    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError>;
    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError>;
    fn cancel_order(&self, id: OrderId) -> Result<(), BrokerError>;
    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError>;
}
```

### Key Types

```rust
pub struct Position {
    pub symbol: Symbol,
    pub quantity: i64,             // Positive = long, negative = short
    pub avg_cost_cents: i64,
    pub market_value_cents: i64,
    pub unrealized_pnl_cents: i64,
}

pub struct Account {
    pub equity_cents: i64,
    pub buying_power_cents: i64,
    pub cash_cents: i64,
    pub gross_position_value_cents: i64,
}

pub struct BrokerOrder {
    pub symbol: Symbol,
    pub side: BrokerSide,          // Buy or Sell
    pub quantity: u64,
    pub order_type: BrokerOrderType,  // Market or Limit(Price)
}

pub struct Quote {
    pub symbol: Symbol,
    pub bid_cents: i64,
    pub ask_cents: i64,
    pub last_cents: i64,
    pub volume: u64,
}
```

### IBKR Adapter

**Feature:** `ibkr`

Connects to TWS/Gateway via the `ibapi` crate (blocking API).

```rust
let mut broker = IbkrBroker::new("127.0.0.1", 4002, 1);  // 4002 = paper, 4001 = live
broker.connect()?;
let positions = broker.positions()?;
let quote = broker.quote(&Symbol::new("AAPL"))?;
```

### Binance Adapter

**Feature:** `binance`

REST API via `reqwest::blocking`. Converts nanobook symbols (e.g., "BTC") to Binance pairs (e.g., "BTCUSDT").

```rust
let mut broker = BinanceBroker::new(api_key, secret_key, true);  // testnet
broker.connect()?;
```

### Python

```python
broker = nanobook.IbkrBroker("127.0.0.1", 4002, client_id=1)
broker.connect()
positions = broker.positions()   # List[Dict] with symbol, quantity, avg_cost_cents, ...
oid = broker.submit_order("AAPL", "buy", 100, order_type="limit", limit_price_cents=185_00)
quote = broker.quote("AAPL")     # Dict with bid_cents, ask_cents, last_cents, volume

broker = nanobook.BinanceBroker(api_key, secret_key, testnet=True, quote_asset="USDT")
```

---

## Risk Engine

**Crate:** `nanobook-risk`

Pre-trade risk validation for single orders and rebalance batches.

### RiskConfig

```rust
pub struct RiskConfig {
    pub max_position_pct: f64,       // Max single position as fraction of equity (default 0.25)
    pub max_order_value_cents: i64,  // Max single order value
    pub max_batch_value_cents: i64,  // Max rebalance batch value
    pub max_leverage: f64,           // Max gross exposure / equity (default 1.5)
    pub max_drawdown_pct: f64,       // Circuit breaker threshold (default 0.20)
    pub allow_short: bool,           // Allow short positions (default true)
    pub max_short_pct: f64,          // Max short exposure fraction (default 0.30)
    pub min_trade_usd: f64,
    pub max_trade_usd: f64,          // Max single trade USD (default 100,000)
}
```

Notes:

- `max_drawdown_pct` is validated at engine construction and preserved in config,
  but not yet used in execution-time checks.

### Single Order Check

```rust
let engine = RiskEngine::new(RiskConfig::default());
let report = engine.check_order(
    &Symbol::new("AAPL"),
    BrokerSide::Buy,
    100,              // quantity
    185_00,           // price in cents
    &account,
    &current_positions,
);

if report.has_failures() {
    // Order violates risk limits — position concentration, short selling, etc.
}
```

### Batch Check

Validates a full rebalance against position limits, leverage, and short exposure:

```rust
let report = engine.check_batch(
    &orders,              // &[(Symbol, BrokerSide, u64, i64)]
    &account,
    &current_positions,
    &target_weights,      // &[(Symbol, f64)]
);
```

### RiskReport

```rust
pub struct RiskReport {
    pub checks: Vec<RiskCheck>,
}

pub struct RiskCheck {
    pub name: &'static str,
    pub status: RiskStatus,  // Pass | Warn | Fail
    pub detail: String,
}

impl RiskReport {
    pub fn has_failures(&self) -> bool;
    pub fn has_warnings(&self) -> bool;
}
```

### Python

```python
risk = nanobook.RiskEngine(max_position_pct=0.25, max_leverage=1.5)

# Single order
checks = risk.check_order("AAPL", "buy", 100, 185_00,
                          equity_cents=1_000_000_00,
                          positions=[("AAPL", 200)])

# Batch (full rebalance)
checks = risk.check_batch(
    orders=[("AAPL", "buy", 100, 185_00), ("MSFT", "sell", 50, 370_00)],
    equity_cents=1_000_000_00,
    positions=[("AAPL", 200), ("MSFT", 100)],
    target_weights=[("AAPL", 0.6), ("MSFT", 0.2)],
)
# Each check: {"name": "...", "status": "Pass|Warn|Fail", "detail": "..."}
```

---

## Rebalancer CLI

**Crate:** `nanobook-rebalancer`

CLI tool that bridges target weights to IBKR execution with risk checks, rate limiting, and audit trail.

### Pipeline

1. Read target weights from `target.json` (output of your optimizer)
2. Connect to IBKR Gateway for live positions, prices, account data
3. Compute CURRENT → TARGET diff (share quantities, limit prices)
4. Run pre-trade risk checks (position limits, leverage, short exposure)
5. Show plan, confirm (or `--force` for automation)
6. Execute limit orders with rate limiting and timeout-based cancellation
7. Reconcile and log to JSONL audit trail

### Commands

```bash
rebalancer status                     # Check IBKR connection
rebalancer positions                  # Show current positions
rebalancer run target.json            # Plan → confirm → execute
rebalancer run target.json --dry-run  # Plan only
rebalancer run target.json --force    # Skip confirmation (cron/automation)
rebalancer reconcile target.json      # Compare actual vs target
```

### target.json

```json
{
  "timestamp": "2026-02-08T15:30:00Z",
  "targets": [
    { "symbol": "AAPL", "weight": 0.40 },
    { "symbol": "MSFT", "weight": 0.30 },
    { "symbol": "SPY",  "weight": -0.10 },
    { "symbol": "QQQ",  "weight": 0.20 }
  ],
  "constraints": {
    "max_position_pct": 0.40,
    "max_leverage": 1.5
  }
}
```

Positive weights are long, negative are short. Symbols absent from the target but present in the account get closed. See `rebalancer/config.toml.example` for the full configuration reference.

---

## Python Bindings

Install: `pip install nanobook` or `cd python && maturin develop --release`

### Exchange

```python
ex = nanobook.Exchange()
result = ex.submit_limit("buy", 10050, 100, "gtc")
result = ex.submit_market("sell", 50)
ex.cancel(result.order_id)
bid, ask = ex.best_bid_ask()
snap = ex.depth(10)
```

### Stop Orders

```python
ex.submit_stop_market("sell", 9500, 100)
ex.submit_stop_limit("buy", 10500, 10600, 100, "gtc")
ex.submit_trailing_stop_market("sell", 9500, 100, "percentage", 0.05)
```

### Portfolio

```python
portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel(commission_bps=10))
portfolio.rebalance_simple([("AAPL", 0.6)], [("AAPL", 150_00)])
portfolio.record_return([("AAPL", 155_00)])
metrics = portfolio.compute_metrics(252.0, 0.0)
```

### Strategy Callback

```python
result = nanobook.run_backtest(
    strategy=lambda bar, prices, portfolio: [("AAPL", 0.5), ("GOOG", 0.5)],
    price_series=[{"AAPL": 150_00, "GOOG": 280_00}] * 252,
    initial_cash=1_000_000_00,
    cost_model=nanobook.CostModel.zero(),
)
```

### ITCH Parser

```python
events = nanobook.parse_itch("data/sample.itch")
```

---

## Book Analytics

### Imbalance

```rust
let snap = exchange.depth(10);
if let Some(imb) = snap.imbalance() {
    // [-1.0, 1.0]: positive = buy pressure
    println!("Imbalance: {imb:.4}");
}
```

### Weighted Midpoint

```rust
if let Some(wmid) = snap.weighted_mid() {
    println!("Weighted mid: {wmid:.2}");
}
```

### VWAP

```rust
if let Some(vwap) = Trade::vwap(exchange.trades()) {
    println!("VWAP: {vwap}");
}
```

---

## Persistence & Serde

### Persistence

**Feature flag:** `persistence` (includes `serde` and `event-log`)

```rust
// Exchange — JSON Lines event sourcing
exchange.save(Path::new("orders.jsonl")).unwrap();
let loaded = Exchange::load(Path::new("orders.jsonl")).unwrap();

// Portfolio — JSON
portfolio.save_json(Path::new("portfolio.json")).unwrap();
let loaded = Portfolio::load_json(Path::new("portfolio.json")).unwrap();
```

### Serde

**Feature flag:** `serde`

All public types derive `Serialize`/`Deserialize`: `Price`, `OrderId`, `TradeId`, `Symbol`, `Side`, `TimeInForce`, `OrderStatus`, `Order`, `Trade`, `Event`, `SubmitResult`, `CancelResult`, `ModifyResult`, `BookSnapshot`, `LevelSnapshot`, `StopOrder`, `Position`, `CostModel`, and more.

---

## CLI Reference

Interactive REPL for the order book:

```bash
cargo run --bin lob
```

| Command | Example |
|---------|---------|
| `buy <price> <qty> [ioc\|fok]` | `buy 100.50 100` |
| `sell <price> <qty> [ioc\|fok]` | `sell 101.00 50 ioc` |
| `market <buy\|sell> <qty>` | `market buy 200` |
| `stop <buy\|sell> <price> <qty>` | `stop buy 105.00 100` |
| `cancel <order_id>` | `cancel 3` |
| `book` / `trades` | Show book or trade history |
| `save <path>` / `load <path>` | Persistence (requires feature) |

---

## Performance

### Benchmarks

Single-threaded (AMD Ryzen / Intel Core):

| Operation | Latency | Throughput | Complexity |
|-----------|---------|------------|------------|
| Submit (no match) | **120 ns** | 8.3M ops/sec | O(log P) |
| Submit (with match) | ~200 ns | 5M ops/sec | O(log P + M) |
| BBO query | **~1 ns** | 1B ops/sec | O(1) |
| Cancel (tombstone) | **170 ns** | 5.9M ops/sec | **O(1)** |
| L2 snapshot (10 levels) | ~500 ns | 2M ops/sec | O(D) |

Where P = price levels, M = orders matched, D = depth.

### Time Breakdown (Submit, No Match)

```
submit_limit() ~120 ns:
├── FxHashMap insert     ~30 ns   order storage
├── BTreeMap insert      ~30 ns   price level (O(log P))
├── VecDeque push         ~5 ns   FIFO queue
├── Event recording      ~10 ns   (optional, for replay)
└── Overhead             ~45 ns   struct creation, etc.
```

### Optimizations

1. **O(1) cancel** — Tombstone-based, 350x faster than linear scan
2. **FxHash** — Non-cryptographic hash for OrderId lookups (+25% vs std HashMap)
3. **Cached BBO** — Best bid/ask cached for O(1) access
4. **Optional event logging** — disable `event-log` feature for max throughput

### Rust vs Numba

Single-threaded throughput is roughly equivalent (both compile to LLVM IR). Where Rust wins: zero cold-start (vs Numba's ~300 ms JIT), true parallelism via Rayon with no GIL contention, and deterministic memory without GC pauses.

---

## Comparison with Other Rust LOBs

| Library | Throughput | Order Types | Deterministic | Use Case |
|---------|------------|-------------|:---:|----------|
| **nanobook** | **8M ops/sec** | Limit, Market, Stops, GTC/IOC/FOK | **Yes** | Strategy backtesting |
| [limitbook](https://lib.rs/crates/limitbook) | 3-5M ops/sec | Limit, Market | No | General purpose |
| [lobster](https://lib.rs/crates/lobster) | ~300K ops/sec | Limit, Market | No | Simple matching |
| [OrderBook-rs](https://github.com/joaquinbejar/OrderBook-rs) | 200K ops/sec | Many (iceberg, peg, etc.) | No | Production HFT |

---

## Design Constraints

Engineering decisions that keep the system simple and fast:

| Constraint | Rationale |
|------------|-----------|
| **Single-threaded** | Deterministic by design — same inputs always produce same outputs |
| **In-process** | No networking overhead; wrap externally if needed |
| **No compliance** | No self-trade prevention or circuit breakers (out of scope) |
| **No complex orders** | No iceberg or pegged orders |
| **Integer prices** | Fixed-point arithmetic avoids floating-point rounding |
| **Statistics in Python** | Spearman/IC/t-stat belong in scipy/Polars — proven, mature |
