# limit-order-book-rs

A deterministic, nanosecond-precision limit order book and matching engine for testing trading algorithms.

## What Is This?

A **simulated stock exchange** that processes orders exactly like a real exchange — with proper price-time priority, partial fills, and cancellations. Built for:

- Testing trading strategies against realistic market microstructure
- Learning how exchanges actually work
- Demonstrating Rust + finance skills for quant roles

## Features

- **Order types**: Limit, Market, Cancel, Modify
- **Time-in-force**: GTC, IOC, FOK
- **Price-time priority**: FIFO matching at each price level
- **Nanosecond timestamps**: Monotonic counter (not system clock)
- **Deterministic**: Same inputs → same outputs (essential for backtesting)
- **Fast**: 8M+ orders/second single-threaded (see Performance)
- **Book snapshots**: L1 (BBO), L2 (depth), L3 (full book)
- **Event replay**: Complete audit trail for deterministic replay

## Quick Example

```rust
use limit_order_book::{Exchange, Side, Price, TimeInForce};

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

Add to `Cargo.toml`:

```toml
[dependencies]
limit-order-book = "0.1"
```

Or build from source:

```bash
git clone https://github.com/ricardofrantz/limit-order-book-rs
cd limit-order-book-rs
cargo build --release
cargo test
cargo bench
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

## Limitations

This is an **educational/testing tool**, not a production exchange:

- **No networking**: In-process only
- **No persistence**: In-memory only
- **No compliance**: Self-trade prevention, circuit breakers
- **No complex orders**: Iceberg, pegged, stop-loss
- **Single symbol**: One order book per Exchange instance

See SPECS.md for the complete specification.

## License

MIT

## Contributing

Issues and PRs welcome. See SPECS.md for the technical specification.
