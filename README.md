# nanobook

[![CI](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml/badge.svg)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/nanobook.svg)](https://crates.io/crates/nanobook)
[![docs.rs](https://docs.rs/nanobook/badge.svg)](https://docs.rs/nanobook)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**n*10⁻⁹** — A deterministic, nanosecond-precision limit order book and matching engine for testing trading algorithms.

## What Is This?

A **simulated stock exchange** that processes orders exactly like a real exchange — with proper price-time priority, partial fills, and cancellations.

### Who Should Use This?

| If you're a... | Use this for... |
|----------------|-----------------|
| **Quant developer** | Backtesting trading strategies with realistic market microstructure |
| **Algo trader** | Testing order execution logic (slippage, queue position, fill rates) |
| **Student** | Learning how exchanges actually work under the hood |
| **Rust developer** | Reference implementation of a financial data structure |

### Why This Library?

- **Deterministic** — Same inputs always produce same outputs (essential for reproducible backtests)
- **Fast** — 8M+ orders/sec single-threaded, sub-microsecond latency
- **Complete** — GTC/IOC/FOK, partial fills, modify, cancel, L1/L2/L3 snapshots
- **Multi-symbol** — `MultiExchange` for independent per-symbol order books
- **Portfolio engine** — Position tracking, cost modeling, rebalancing, financial metrics (Sharpe, Sortino, max drawdown)
- **Trailing stops** — Fixed, percentage, and ATR-based adaptive trailing
- **Python bindings** — Full API via PyO3 (`pip install nanobook`)
- **Simple** — Single-threaded, in-process, minimal dependencies

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
- **Price-time priority**: FIFO matching at each price level
- **Nanosecond timestamps**: Monotonic counter (not system clock)
- **Deterministic**: Same inputs → same outputs (essential for backtesting)
- **Fast**: 8M+ orders/second single-threaded (see Performance)
- **Book snapshots**: L1 (BBO), L2 (depth), L3 (full book), imbalance, weighted mid
- **Event replay**: Complete audit trail for deterministic replay
- **Portfolio**: Position tracking, VWAP entry, cost model, Sharpe/Sortino/drawdown metrics
- **Strategy trait**: Define `compute_weights()`, run backtests with `run_backtest()`
- **Multi-symbol**: Independent order books per symbol via `MultiExchange`
- **Parallel sweeps**: Rayon-based parameter grid search (optional feature)
- **Python bindings**: Full API via PyO3 with GIL release for parallel sweeps

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
nanobook = "0.4"
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

# Portfolio management
portfolio = nanobook.Portfolio(1_000_000_00, nanobook.CostModel.zero())
portfolio.rebalance_simple([("AAPL", 0.5)], [("AAPL", 150_00)])
portfolio.record_return([("AAPL", 155_00)])
metrics = portfolio.compute_metrics(12.0, 0.0)
print(f"Sharpe: {metrics.sharpe:.2f}")

# Parallel parameter sweep (GIL released for Rayon)
results = nanobook.py_sweep_equal_weight(
    n_params=100,
    price_series=[[("AAPL", p)] for p in range(150_00, 160_00)],
    initial_cash=1_000_000_00,
    periods_per_year=252.0,
)
```

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

// Cancel an order
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
BIDS (sorted high→low)          ASKS (sorted low→high)

$100.00: [O1]→[O2]→[O3]         $100.50: [O7]→[O8]
$99.50:  [O4]→[O5]              $101.00: [O9]
$99.00:  [O6]                   $102.00: [O10]→[O11]

        ↑ Best Bid              ↑ Best Ask
```

- **BTreeMap<Price, Level>** for sorted price levels
- **VecDeque<Order>** for FIFO queue at each level
- **HashMap<OrderId, OrderRef>** for O(1) lookup/cancel
- **Cached best_price** for O(1) BBO access

### Matching Algorithm

1. Incoming order checks opposite side of book
2. If prices cross (buy ≥ best ask, or sell ≤ best bid), match
3. Fill at resting order's price (price improvement for aggressor)
4. Continue until no more crosses or order fully filled
5. Remaining quantity: rests (GTC), cancels (IOC/Market), or entire order cancels (FOK)

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
| BBO query | **1 ns** | 1B ops/sec | O(1) |
| Cancel | 660 ns† | 1.5M ops/sec | O(N) |
| L2 snapshot (10 levels) | ~500 ns | 2M ops/sec | O(D) |

Where P = price levels, M = orders matched, N = orders at price level, D = depth.

†Cancel is O(N) in orders at that price level. See "Future Optimizations" below.

```bash
cargo bench
```

### Optimizations Applied

1. **FxHash** — Non-cryptographic hash for OrderId lookups (+25% vs std HashMap)
2. **Cached BBO** — Best bid/ask cached for O(1) access
3. **Optional event logging** — Disable for max throughput:

```bash
# With event logging (default) - enables replay
cargo build --release

# Without event logging - maximum performance
cargo build --release --no-default-features
```

### Where Time Goes (Submit, No Match)

```
submit_limit() ~120 ns breakdown:
├── FxHashMap insert     ~30 ns   order storage
├── BTreeMap insert      ~30 ns   price level (O(log P))
├── VecDeque push        ~5 ns    FIFO queue
├── Event recording      ~10 ns   (optional, for replay)
└── Overhead             ~45 ns   struct creation, etc.
```

### Future Optimizations

| Optimization | Potential Gain | Complexity | Tradeoff |
|--------------|----------------|------------|----------|
| **O(1) cancel** | 10x for deep levels | High | Intrusive linked list or tombstones |
| **Array-indexed levels** | -30 ns | Medium | Requires bounded price range |
| **Slab allocator** | -10 ns | Medium | More complex memory management |

**O(1) Cancel**: Currently cancel scans the VecDeque to find the order. For true O(1):
- Tombstone approach: mark cancelled, skip during matching
- Intrusive doubly-linked list with HashMap<OrderId, NodePtr>

These add complexity. Current O(N) cancel is fine unless you have thousands of orders at a single price level (rare in practice).

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

### IOC for Aggressive Execution

```rust
// Take liquidity without resting an order
let result = exchange.submit_limit(Side::Buy, Price(100_50), 1000, TimeInForce::IOC);
// Fills what's available at ≤$100.50, remainder cancelled
println!("Filled: {}, Cancelled: {}",
    result.trades.iter().map(|t| t.quantity).sum::<u64>(),
    exchange.get_order(result.order_id).map(|o| o.remaining_quantity).unwrap_or(0)
);
```

## Comparison with Other Rust LOBs

| Library | Throughput | Threading | Order Types | Deterministic | Use Case |
|---------|------------|-----------|-------------|---------------|----------|
| **nanobook** (this) | **8M ops/sec** | Single | Limit, Market, GTC/IOC/FOK | **Yes** | Backtesting, education |
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
| `portfolio` | No | Portfolio engine, position tracking, metrics |
| `parallel` | No | Rayon-based parallel parameter sweeps |

## Limitations

This is an **educational/testing tool**, not a production exchange:

- **No networking**: In-process only
- **No compliance**: Self-trade prevention, circuit breakers
- **No complex orders**: Iceberg, pegged orders

See SPECS.md for the complete specification.

## License

MIT

## Contributing

Issues and PRs welcome. See SPECS.md for the technical specification.

### Recording a Demo GIF

To create an animated GIF of the demo (for docs, presentations, etc.):

```bash
# Using vhs (recommended): https://github.com/charmbracelet/vhs
vhs examples/demo.tape

# Using asciinema + agg:
asciinema rec demo.cast -c "cargo run --example demo_quick"
agg demo.cast demo.gif
```
