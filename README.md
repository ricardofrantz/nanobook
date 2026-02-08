# nanobook

[![CI](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml/badge.svg)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/nanobook.svg)](https://crates.io/crates/nanobook)
[![docs.rs](https://docs.rs/nanobook/badge.svg)](https://docs.rs/nanobook)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**n*10⁻⁹** — A deterministic, nanosecond-precision limit order book and matching engine for testing trading algorithms.

## What Is This?

A **simulated stock exchange** that processes orders exactly like a real exchange — with proper price-time priority, partial fills, and cancellations.

nanobook combines five layers: a **LOB matching engine** for deterministic order matching, a **portfolio simulator** for position tracking and strategy backtesting, a **rebalancer** that executes portfolio changes through Interactive Brokers, **Python bindings** for seamless integration with data science workflows, and a **NASDAQ ITCH 5.0 parser** for replaying real market data. Each layer works independently or together.

### Who Should Use This?

| If you're a... | Use this for... |
|----------------|-----------------|
| **Quant developer** | Backtesting trading strategies with realistic market microstructure |
| **Algo trader** | Testing order execution logic (slippage, queue position, fill rates) |
| **Portfolio manager** | Automated IBKR rebalancing with risk checks and audit trail |
| **Student** | Learning how exchanges actually work under the hood |
| **Rust developer** | Reference implementation of a financial data structure |

### Why This Library?

- **Deterministic** — Same inputs always produce same outputs (essential for reproducible backtests)
- **Fast** — 8M+ orders/sec single-threaded, O(1) cancel via tombstones (170 ns)
- **Complete** — GTC/IOC/FOK, partial fills, modify, cancel, L1/L2/L3 snapshots
- **Stop orders** — Stop-market, stop-limit, trailing stops (fixed/percentage/ATR)
- **Multi-symbol** — `MultiExchange` for independent per-symbol order books
- **Portfolio engine** — Position tracking, cost modeling, rebalancing, financial metrics (Sharpe, Sortino, max drawdown)
- **ITCH 5.0 parser** — Replay real NASDAQ market data through the matching engine
- **IBKR rebalancer** — CLI tool to execute portfolio changes via Interactive Brokers with risk checks, rate limiting, and JSONL audit trail
- **Python bindings** — PyO3 wrappers for Exchange, Portfolio, sweep (`pip install nanobook`)
- **Simple** — Single-threaded, in-process, minimal dependencies (3 core deps)

### Why Not Something Else?

| Framework | Problem |
|-----------|---------|
| VectorBT | Per-asset signals, no cross-sectional ranking |
| Zipline | Abandoned, bundle pain |
| Backtrader | Slow (pure Python), verbose |
| HFTBacktest | Crypto tick-level, wrong abstraction |
| Nautilus Trader | Enterprise complexity |

nanobook's niche: lightweight, factor-strategy-optimized portfolio simulator
with an optional LOB execution layer and IBKR rebalancer. Lean (3 deps),
fast (8M+ orders/sec), Python-friendly.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                      Python (PyO3)                           │
│     Factor ranking, strategy logic, Polars DataFrames        │
├────────────────┬──────────────────────┬──────────────────────┤
│                │                      │                      │
│  Portfolio     │   LOB Exchange       │   Market Data        │
│  ───────────   │   ────────────       │   ───────────        │
│  Position      │   OrderBook          │   ITCH 5.0 Parser    │
│  CostModel  ◄─┼─► Matching Engine    │   Event Conversion   │
│  Strategy      │   Stop / Trailing    │   Memory-Mapped I/O  │
│  Sweep (‖)     │   Event Replay       │                      │
│                │   O(1) Cancel        │                      │
├────────────────┴──────────────────────┴──────────────────────┤
│   Shared: Symbol, MultiExchange, BookSnapshot, Analytics     │
├──────────────────────────────────────────────────────────────┤
│                   Rebalancer (CLI)                            │
│     target.json → IBKR live positions/prices → diff →        │
│     risk checks → limit orders → audit trail (JSONL)         │
└──────────────────────────────────────────────────────────────┘
```

See [DOC.md](DOC.md) for the full API reference.

## See It In Action

```bash
cargo run --example demo        # Interactive walkthrough with explanations
cargo run --example demo_quick  # Quick non-interactive demo
```

```
Building order book...
  SELL 100 @ $50.25 (Alice)       ASK  $50.50   150 shares
  SELL 150 @ $50.50 (Bob)         ASK  $50.25   100 shares
  BUY  100 @ $50.00 (Carol)       ---- spread: $0.25 ----
  BUY  200 @ $49.75 (Dan)         BID  $50.00   100 shares
                                  BID  $49.75   200 shares

Incoming: BUY 120 @ $50.25 (Eve) - CROSSES SPREAD!
  Trades: 100 shares @ $50.25    (Alice filled completely)
  Filled: 100, Resting: 20       (Eve's remainder rests on book)

Incoming: MARKET BUY 200 (Frank) - SWEEPS THE BOOK!
  Trades: 150 shares @ $50.50    (Bob filled completely)
  Unfilled: 50                   (no more liquidity!)
```

The interactive demo explains price-time priority, partial fills, IOC/FOK, and order cancellation.

## Features

- **Order types**: Limit, Market, Cancel, Modify
- **Time-in-force**: GTC, IOC, FOK
- **Stop orders**: Stop-market, stop-limit, trailing stops (fixed/percentage/ATR)
- **O(1) cancel**: Tombstone-based cancellation (~170 ns, 350x faster than v0.5)
- **Price-time priority**: FIFO matching at each price level
- **Nanosecond timestamps**: Monotonic counter (not system clock)
- **Deterministic**: Same inputs -> same outputs (essential for backtesting)
- **Fast**: 8M+ orders/second single-threaded (see Performance)
- **Book snapshots**: L1 (BBO), L2 (depth), L3 (full book), imbalance, weighted mid
- **Event replay**: Complete audit trail for deterministic replay
- **Portfolio**: Position tracking, VWAP entry, cost model, Sharpe/Sortino/drawdown metrics
- **Strategy trait**: Define `compute_weights()`, run backtests with `run_backtest()`
- **Multi-symbol**: Independent order books per symbol via `MultiExchange`
- **Parallel sweeps**: Rayon-based parameter grid search (optional feature)
- **ITCH 5.0 parser**: Streaming binary parser for NASDAQ market data
- **IBKR rebalancer**: CLI tool for automated portfolio rebalancing via Interactive Brokers (risk checks, rate limiting, JSONL audit)
- **Python bindings**: Exchange, Portfolio, sweep via PyO3 with GIL release for parallel sections

## Quick Example

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

fn main() {
    let mut exchange = Exchange::new();

    // Alice sells 100 shares at $50.00
    let alice = exchange.submit_limit(Side::Sell, Price(50_00), 100, TimeInForce::GTC);

    // Bob sells 100 shares at $51.00
    let bob = exchange.submit_limit(Side::Sell, Price(51_00), 100, TimeInForce::GTC);

    // Charlie buys 150 shares at $51.00 — crosses the book!
    let result = exchange.submit_limit(Side::Buy, Price(51_00), 150, TimeInForce::GTC);

    // Two trades execute:
    // 1. Charlie buys 100 from Alice at $50.00 (best price)
    // 2. Charlie buys 50 from Bob at $51.00
    assert_eq!(result.trades.len(), 2);
    assert_eq!(result.trades[0].price, Price(50_00));
    assert_eq!(result.trades[0].quantity, 100);
    assert_eq!(result.trades[1].price, Price(51_00));
    assert_eq!(result.trades[1].quantity, 50);
}
```

## Installation

### Rust

Add to `Cargo.toml`:

```toml
[dependencies]
nanobook = "0.6"
```

### Python

```bash
pip install nanobook
```

Or build from source:

```bash
git clone https://github.com/ricardofrantz/nanobook
cd nanobook
cargo build --release
cargo test
cargo bench

# Rebalancer CLI
cargo build -p nanobook-rebalancer --release

# Python bindings
cd python && maturin develop --release
```

## Python Quick Start

```python
import nanobook

# Create an exchange
ex = nanobook.Exchange()

# Submit orders (strings for enums, ints for prices in cents)
ex.submit_limit("sell", 10050, 100, "gtc")   # sell 100 @ $100.50
result = ex.submit_limit("buy", 10050, 100, "gtc")
assert result.status == "Filled"
assert result.trades[0].price == 10050

# Market orders, stops, trailing stops
ex.submit_market("buy", 50)
ex.submit_stop_market("sell", 9500, 100)
ex.submit_trailing_stop_market("sell", 9500, 100, "percentage", 0.05)

# Book queries
bid, ask = ex.best_bid_ask()
snap = ex.depth(10)

# Compact tombstones after heavy cancellation
ex.compact()

# Portfolio management
portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
portfolio.rebalance_simple([("AAPL", 0.5)], [("AAPL", 150_00)])
portfolio.record_return([("AAPL", 155_00)])
metrics = portfolio.compute_metrics(12.0, 0.0)
print(f"Sharpe: {metrics.sharpe:.2f}")

# Custom strategy via callback
result = nanobook.run_backtest(
    strategy=lambda bar, prices, portfolio: [("AAPL", 0.5), ("GOOG", 0.5)],
    price_series=[{"AAPL": 150_00, "GOOG": 280_00}] * 252,
    initial_cash=1_000_000_00,
    cost_model=nanobook.CostModel.zero(),
)

# Parse NASDAQ ITCH 5.0 binary data
events = nanobook.parse_itch("data/sample.itch")
```

## Rebalancer

The `rebalancer/` crate is a CLI tool that bridges nanobook's portfolio math to [Interactive Brokers](https://www.interactivebrokers.com/) for real (or paper) order execution.

```bash
# Build
cargo build -p nanobook-rebalancer --release

# Check IBKR connection
rebalancer status

# Show current positions
rebalancer positions

# Compute diff, show plan, confirm, execute
rebalancer run target.json

# Dry run (plan only, no orders)
rebalancer run target.json --dry-run

# Skip confirmation (for cron/automation)
rebalancer run target.json --force

# Compare actual positions vs target
rebalancer reconcile target.json
```

### How It Works

1. Reads target weights from a `target.json` (output of your optimizer/bot)
2. Connects to IBKR Gateway for live positions, prices, and account data
3. Computes the CURRENT → TARGET diff (share quantities, limit prices)
4. Runs pre-trade risk checks (position limits, leverage, short exposure)
5. Shows the plan and asks for Y/N confirmation
6. Executes limit orders with rate limiting and timeout-based cancellation
7. Reconciles actual vs target and logs everything to a JSONL audit trail

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

## API Overview

### Core Types

```rust
/// Price in smallest units (e.g., cents). Price(10050) = $100.50
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(pub i64);

/// Order side
pub enum Side { Buy, Sell }

/// Time-in-force: how long an order stays active
pub enum TimeInForce {
    GTC,  // Good-til-cancelled: rests on book until filled or cancelled
    IOC,  // Immediate-or-cancel: fill what you can, cancel the rest
    FOK,  // Fill-or-kill: fill entirely or cancel entirely
}

/// Order status
pub enum OrderStatus {
    New,              // Accepted, resting on book
    PartiallyFilled,  // Some quantity filled, rest on book
    Filled,           // Fully executed
    Cancelled,        // Removed by user or TIF
}
```

### Exchange Operations

```rust
let mut exchange = Exchange::new();

// Submit limit order (returns order ID + any immediate trades)
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
println!("Order ID: {:?}, Trades: {}", result.order_id, result.trades.len());

// Submit market order (always IOC semantics)
let result = exchange.submit_market(Side::Sell, 50);

// Cancel an order — O(1) via tombstones
let cancel_result = exchange.cancel(order_id);

// Modify an order (cancel + replace, loses time priority)
let modify_result = exchange.modify(order_id, Price(101_00), 200);

// Get order status
if let Some(order) = exchange.get_order(order_id) {
    println!("Remaining: {}", order.remaining_quantity);
}

// Book snapshots
let (best_bid, best_ask) = exchange.best_bid_ask();  // L1
let depth = exchange.depth(10);                       // L2: top 10 levels
let full = exchange.full_book();                      // L3: everything

// Compact tombstones after heavy cancellation workloads
exchange.compact();
```

### Stop Orders

```rust
// Stop-market: triggers market order when price hits stop
exchange.submit_stop_market(Side::Sell, Price(95_00), 100);

// Stop-limit: triggers limit order at limit_price when stop hits
exchange.submit_stop_limit(Side::Sell, Price(95_00), Price(94_50), 100, TimeInForce::GTC);

// Trailing stops: stop price tracks the market
exchange.submit_trailing_stop_market(Side::Sell, Price(95_00), 100, TrailMethod::Fixed(1_00));
exchange.submit_trailing_stop_market(Side::Sell, Price(95_00), 100, TrailMethod::Percentage(0.05));
exchange.submit_trailing_stop_market(Side::Sell, Price(95_00), 100,
    TrailMethod::Atr { multiplier: 2.0, period: 14 });
```

### Result Types

```rust
pub struct SubmitResult {
    pub order_id: OrderId,
    pub status: OrderStatus,
    pub trades: Vec<Trade>,
}

pub struct Trade {
    pub id: TradeId,
    pub price: Price,
    pub quantity: Quantity,
    pub aggressor_order_id: OrderId,
    pub passive_order_id: OrderId,
    pub aggressor_side: Side,
    pub timestamp: Timestamp,
}
```

## How It Works

### Order Book Structure

```
BIDS (sorted high->low)          ASKS (sorted low->high)

$100.00: [O1]->[O2]->[O3]         $100.50: [O7]->[O8]
$99.50:  [O4]->[O5]              $101.00: [O9]
$99.00:  [O6]                   $102.00: [O10]->[O11]

        ^ Best Bid              ^ Best Ask
```

- **BTreeMap<Price, Level>** for sorted price levels
- **VecDeque<OrderId>** for FIFO queue at each level (tombstones for O(1) cancel)
- **FxHashMap<OrderId, Order>** for O(1) lookup
- **Cached best_price** for O(1) BBO access

### Matching Algorithm

1. Incoming order checks opposite side of book
2. If prices cross (buy >= best ask, or sell <= best bid), match
3. Fill at resting order's price (price improvement for aggressor)
4. Continue until no more crosses or order fully filled
5. Remaining quantity: rests (GTC), cancels (IOC/Market), or entire order cancels (FOK)

### Cancellation (v0.6)

Orders are cancelled by marking them as tombstones in the VecDeque rather than removing them.
This avoids an O(N) scan and shift, reducing deep-level cancel latency from ~60 us to ~170 ns.
Tombstones are automatically skipped during matching. Call `exchange.compact()` to reclaim memory
after heavy cancellation workloads.

### Time-in-Force Behavior

| TIF | Partial Fill OK? | Rests on Book? |
|-----|------------------|----------------|
| GTC | Yes | Yes (remainder) |
| IOC | Yes | No (remainder cancelled) |
| FOK | No | No (all-or-nothing) |

### Determinism

- No randomness anywhere
- Timestamps from monotonic counter, not system clock
- Same order sequence always produces same trades
- Event log enables exact replay

## Performance

### Benchmarks

Measured on AMD Ryzen / Intel Core (single-threaded):

| Operation | Time | Throughput | Complexity |
|-----------|------|------------|------------|
| Submit (no match) | **120 ns** | 8.3M ops/sec | O(log P) |
| Submit (with match) | ~200 ns | 5M ops/sec | O(log P + M) |
| BBO query | **~1 ns** | 1B ops/sec | O(1) |
| Cancel | **170 ns** | 5.9M ops/sec | **O(1)** |
| L2 snapshot (10 levels) | ~500 ns | 2M ops/sec | O(D) |

Where P = price levels, M = orders matched, D = depth.

```bash
cargo bench
```

### Optimizations Applied

1. **O(1) cancel** — Tombstone-based cancellation, 350x faster than linear scan
2. **FxHash** — Non-cryptographic hash for OrderId lookups (+25% vs std HashMap)
3. **Cached BBO** — Best bid/ask cached for O(1) access
4. **Optional event logging** — Disable for max throughput:

```bash
# With event logging (default) - enables replay
cargo build --release

# Without event logging - maximum performance
cargo build --release --no-default-features
```

### Where Time Goes (Submit, No Match)

```
submit_limit() ~120 ns breakdown:
+-- FxHashMap insert     ~30 ns   order storage
+-- BTreeMap insert      ~30 ns   price level (O(log P))
+-- VecDeque push        ~5 ns    FIFO queue
+-- Event recording      ~10 ns   (optional, for replay)
+-- Overhead             ~45 ns   struct creation, etc.
```

### Rust vs Numba

Single-threaded throughput is roughly equivalent (both compile to LLVM IR). Where Rust wins: zero cold-start (vs Numba's ~300 ms JIT), true parallelism via Rayon with no GIL contention, and deterministic memory without GC pauses.

## Use Cases

### Strategy Backtesting

```rust
for event in historical_events {
    let result = exchange.apply(&event);
    strategy.on_result(&result);
    strategy.on_book_update(exchange.best_bid_ask());
}
```

### Market Impact Analysis

```rust
let (bid_before, _) = exchange.best_bid_ask();
let result = exchange.submit_market(Side::Buy, 10_000);
let (bid_after, _) = exchange.best_bid_ask();
let slippage = bid_after.unwrap().0 - bid_before.unwrap().0;
```

### Queue Position Testing

```rust
// Who's first in line at $100?
let competitor = exchange.submit_limit(Side::Buy, Price(100_00), 1000, TimeInForce::GTC);
let mine = exchange.submit_limit(Side::Buy, Price(100_00), 1000, TimeInForce::GTC);

// Sell comes in — who gets filled?
exchange.submit_limit(Side::Sell, Price(100_00), 500, TimeInForce::GTC);

// Competitor was first, gets filled first
let comp_order = exchange.get_order(competitor.order_id).unwrap();
let my_order = exchange.get_order(mine.order_id).unwrap();
assert_eq!(comp_order.filled_quantity, 500);
assert_eq!(my_order.filled_quantity, 0);
```

### ITCH Replay

```rust
use nanobook::itch::{ItchParser, ItchMessage};

let file = std::fs::File::open("data/sample.itch")?;
let mut parser = ItchParser::new(std::io::BufReader::new(file));

while let Some(msg) = parser.next_message()? {
    match msg {
        ItchMessage::AddOrder { stock, side, shares, price, .. } => {
            println!("{stock}: {side:?} {shares} @ {price}");
        }
        _ => {}
    }
}
```

## Comparison with Other Rust LOBs

| Library | Throughput | Threading | Order Types | Deterministic | Use Case |
|---------|------------|-----------|-------------|---------------|----------|
| **nanobook** (this) | **8M ops/sec** | Single | Limit, Market, GTC/IOC/FOK, Stops | **Yes** | Backtesting, education |
| [limitbook](https://lib.rs/crates/limitbook) | 3-5M ops/sec | Single | Limit, Market | No | General purpose |
| [lobster](https://lib.rs/crates/lobster) | ~300K ops/sec | Single | Limit, Market | No | Simple matching |
| [OrderBook-rs](https://github.com/joaquinbejar/OrderBook-rs) | 200K ops/sec | **Multi** | Many (iceberg, peg, etc.) | No | Production HFT |

**When to use what:**

- **This library**: You need deterministic replay for backtesting, or you're learning how exchanges work
- **limitbook**: General-purpose LOB without replay requirements
- **OrderBook-rs**: Production systems needing thread-safety and complex order types

## Feature Flags

| Feature | Default | Description |
|---------|---------|-------------|
| `event-log` | Yes | Event recording for deterministic replay |
| `serde` | No | Serialize/deserialize all public types |
| `persistence` | No | File-based event sourcing (JSON Lines) |
| `portfolio` | No | Portfolio engine, position tracking, metrics, strategy trait |
| `parallel` | No | Rayon-based parallel parameter sweeps |
| `itch` | No | NASDAQ ITCH 5.0 binary protocol parser |

## Non-Goals

| Temptation | Why Not |
|-----------|---------|
| Spearman/IC/t-stat in Rust | Use scipy/Polars — proven, mature |
| arrow-rs dependency | 2 -> 20+ deps. PyO3 handles data exchange |
| Event-driven Strategy trait | Wrong pattern for factor strategies. Batch vectorized |
| Replace VectorBT | VectorBT is fine for ad-hoc research. nanobook = production |
| Networking/WebSocket | In-process only. Wrap externally if needed |
| GUI/dashboard | Use Python (Streamlit/Jupyter) for visualization |

## Limitations

This is an **educational/testing tool**, not a production exchange:

- **No networking**: In-process only
- **No compliance**: Self-trade prevention, circuit breakers
- **No complex orders**: Iceberg, pegged orders

## License

MIT

## Contributing

Issues and PRs welcome.

### Recording a Demo GIF

To create an animated GIF of the demo (for docs, presentations, etc.):

```bash
# Using vhs (recommended): https://github.com/charmbracelet/vhs
vhs examples/demo.tape

# Using asciinema + agg:
asciinema rec demo.cast -c "cargo run --example demo_quick"
agg demo.cast demo.gif
```
