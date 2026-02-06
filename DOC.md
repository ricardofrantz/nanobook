# nanobook

[![CI](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml/badge.svg)](https://github.com/ricardofrantz/nanobook/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/nanobook.svg)](https://crates.io/crates/nanobook)
[![docs.rs](https://docs.rs/nanobook/badge.svg)](https://docs.rs/nanobook)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

**Developer Reference** — Deterministic limit order book and matching engine for testing trading algorithms.

---

## Table of Contents

- [Quick Start](#quick-start)
- [Core Concepts](#core-concepts)
- [API Reference](#api-reference)
  - [Exchange](#exchange)
  - [Types](#types)
  - [Enums](#enums)
  - [Result Types](#result-types)
  - [Snapshots](#snapshots)
  - [Advanced Types](#advanced-types)
  - [Event Replay](#event-replay)
- [Symbol](#symbol)
- [MultiExchange](#multiexchange)
- [Portfolio Engine](#portfolio-engine)
- [Book Analytics](#book-analytics)
- [Time-in-Force Semantics](#time-in-force-semantics)
- [CLI Reference](#cli-reference)
- [Performance](#performance)
- [Input Validation](#input-validation)
- [Stop Orders](#stop-orders)
- [Trailing Stops](#trailing-stops)
- [Strategy Trait](#strategy-trait)
- [Persistence](#persistence)
- [Serde Support](#serde-support)
- [Python Bindings](#python-bindings)
- [Common Patterns](#common-patterns)
- [Limitations](#limitations)

---

## Quick Start

Add to `Cargo.toml`:

```toml
[dependencies]
nanobook = "0.4"
```

Minimal working example:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

fn main() {
    let mut exchange = Exchange::new();

    // Post a sell order: 100 shares at $50.00
    exchange.submit_limit(Side::Sell, Price(50_00), 100, TimeInForce::GTC);

    // Post a buy order that crosses the spread
    let result = exchange.submit_limit(Side::Buy, Price(50_00), 100, TimeInForce::GTC);

    assert_eq!(result.trades.len(), 1);
    assert_eq!(result.trades[0].price, Price(50_00));
    assert_eq!(result.trades[0].quantity, 100);
}
```

---

## Core Concepts

### Prices

Prices are integers in the **smallest currency unit** (e.g., cents for USD). This avoids floating-point rounding errors.

```rust
use nanobook::Price;

let price = Price(100_50);  // $100.50
let penny = Price(1);       // $0.01

// Arithmetic works as expected
assert!(Price(100_00) < Price(101_00));

// Special values
let _ = Price::ZERO;  // Price(0)
let _ = Price::MAX;   // Price(i64::MAX) — used internally for market buys
let _ = Price::MIN;   // Price(i64::MIN) — used internally for market sells
```

Display formats as dollars: `Price(10050)` prints as `$100.50`.

### Quantities

Quantities are `u64` — always positive, representing shares or contracts.

```rust
use nanobook::Quantity;

let qty: Quantity = 100;  // 100 shares
```

### Timestamps

Timestamps are `u64` nanosecond counters, **not system clock time**. They increment monotonically with each order, guaranteeing deterministic ordering.

```rust
use nanobook::Timestamp;

let ts: Timestamp = 42;  // 42nd nanosecond tick
```

### Determinism

The exchange is fully deterministic:

- No randomness anywhere in the matching engine
- Timestamps come from an internal counter, not `SystemTime`
- Same sequence of operations always produces identical trades
- Event replay reconstructs exact state (see [Event Replay](#event-replay))

This makes nanobook suitable for reproducible backtesting — run your strategy twice, get the same results.

---

## API Reference

### Exchange

`Exchange` is the main entry point. It wraps an `OrderBook` and provides order submission, cancellation, modification, queries, and trade history.

```rust
use nanobook::Exchange;

let mut exchange = Exchange::new();  // or Exchange::default()
```

#### Order Submission

##### `submit_limit`

```rust
pub fn submit_limit(
    &mut self,
    side: Side,
    price: Price,
    quantity: Quantity,
    tif: TimeInForce,
) -> SubmitResult
```

Submit a limit order. The order is matched against the opposite side of the book. Remaining quantity is handled according to time-in-force (see [Time-in-Force Semantics](#time-in-force-semantics)).

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

// GTC: rests on book if not filled
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
assert!(result.is_resting());

// IOC: fill what's available, cancel remainder
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);
assert!(!result.is_resting());  // IOC never rests

// FOK: all-or-nothing
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);
// Either fully filled or entirely cancelled
```

##### `submit_market`

```rust
pub fn submit_market(&mut self, side: Side, quantity: Quantity) -> SubmitResult
```

Submit a market order. Executes immediately at the best available prices. Unfilled quantity is cancelled (IOC semantics). Internally, this is a limit order at `Price::MAX` (buy) or `Price::MIN` (sell) with `TimeInForce::IOC`.

```rust
let result = exchange.submit_market(Side::Buy, 500);
println!("Filled: {}, Cancelled: {}", result.filled_quantity, result.cancelled_quantity);
```

#### Order Management

##### `cancel`

```rust
pub fn cancel(&mut self, order_id: OrderId) -> CancelResult
```

Cancel an active order. Returns the cancelled quantity on success.

```rust
use nanobook::OrderId;

let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
let result = exchange.cancel(submit.order_id);

if result.success {
    println!("Cancelled {} shares", result.cancelled_quantity);
} else {
    println!("Error: {:?}", result.error);
}
```

Possible errors:
- `CancelError::OrderNotFound` — no order with that ID exists
- `CancelError::OrderNotActive` — order is already filled or cancelled

##### `modify`

```rust
pub fn modify(
    &mut self,
    order_id: OrderId,
    new_price: Price,
    new_quantity: Quantity,
) -> ModifyResult
```

Modify an order via cancel-and-replace. The old order is cancelled and a new order is submitted. The new order:
- Gets a **new OrderId**
- **Loses time priority** (goes to the back of the queue)
- **Inherits** the original order's time-in-force
- May **trade immediately** if the new price crosses the spread

```rust
let submit = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

// Move to a better price with more size
let result = exchange.modify(submit.order_id, Price(101_00), 200);

if result.success {
    let new_id = result.new_order_id.unwrap();
    println!("Old #{} -> New #{}", result.old_order_id.0, new_id.0);
    println!("Trades from modify: {}", result.trades.len());
}
```

Possible errors:
- `ModifyError::OrderNotFound` — no order with that ID exists
- `ModifyError::OrderNotActive` — order is already filled or cancelled
- `ModifyError::InvalidQuantity` — new quantity is zero

#### Queries

```rust
// Single order lookup
let order: Option<&Order> = exchange.get_order(OrderId(1));

// Best bid/ask (L1) — O(1)
let (bid, ask) = exchange.best_bid_ask();
let bid: Option<Price> = exchange.best_bid();
let ask: Option<Price> = exchange.best_ask();

// Spread in smallest units — O(1)
let spread: Option<i64> = exchange.spread();

// Top N levels each side (L2)
let snapshot: BookSnapshot = exchange.depth(10);

// Full book (L3)
let full: BookSnapshot = exchange.full_book();

// Trade history
let trades: &[Trade] = exchange.trades();

// Underlying OrderBook (advanced)
let book: &OrderBook = exchange.book();
```

#### Memory Management

For long-running instances, periodically clear accumulated history:

```rust
// Clear trade history
exchange.clear_trades();

// Remove filled/cancelled orders, keep active ones
let removed: usize = exchange.clear_order_history();
```

---

### Types

| Type | Definition | Description | Display |
|------|-----------|-------------|---------|
| `Price` | `struct Price(pub i64)` | Price in smallest units (cents) | `$100.50` |
| `Quantity` | `type Quantity = u64` | Number of shares/contracts | — |
| `OrderId` | `struct OrderId(pub u64)` | Unique order identifier | `O42` |
| `TradeId` | `struct TradeId(pub u64)` | Unique trade identifier | `T7` |
| `Timestamp` | `type Timestamp = u64` | Nanosecond counter (not wall clock) | — |

#### Price Constants

```rust
Price::ZERO  // Price(0)
Price::MAX   // Price(i64::MAX)
Price::MIN   // Price(i64::MIN)
```

`Price` implements `Clone`, `Copy`, `Debug`, `Default`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`, `Hash`.

---

### Enums

#### `Side`

```rust
pub enum Side {
    Buy,
    Sell,
}
```

| Method | Returns |
|--------|---------|
| `side.opposite()` | `Buy` ↔ `Sell` |

Display: `"BUY"` or `"SELL"`.

#### `TimeInForce`

```rust
pub enum TimeInForce {
    GTC,  // Good-til-cancelled (default)
    IOC,  // Immediate-or-cancel
    FOK,  // Fill-or-kill
}
```

| Method | GTC | IOC | FOK |
|--------|-----|-----|-----|
| `tif.can_rest()` | `true` | `false` | `false` |
| `tif.allows_partial()` | `true` | `true` | `false` |

Display: `"GTC"`, `"IOC"`, or `"FOK"`. Default: `GTC`.

See [Time-in-Force Semantics](#time-in-force-semantics) for detailed behavior.

#### `OrderStatus`

```rust
pub enum OrderStatus {
    New,              // Accepted, resting on book
    PartiallyFilled,  // Some fills, remainder on book
    Filled,           // Fully executed
    Cancelled,        // Removed by user or TIF rules
}
```

| Method | New | PartiallyFilled | Filled | Cancelled |
|--------|-----|-----------------|--------|-----------|
| `status.is_active()` | `true` | `true` | `false` | `false` |
| `status.is_terminal()` | `false` | `false` | `true` | `true` |

Default: `New`.

---

### Result Types

#### `SubmitResult`

Returned by `submit_limit` and `submit_market`.

```rust
pub struct SubmitResult {
    pub order_id: OrderId,          // Assigned order ID
    pub status: OrderStatus,        // Current status after submission
    pub trades: Vec<Trade>,         // Trades that occurred
    pub filled_quantity: Quantity,   // Total quantity filled
    pub resting_quantity: Quantity,  // Quantity resting on book (GTC only)
    pub cancelled_quantity: Quantity, // Quantity cancelled (IOC/FOK remainder)
}
```

| Method | Description |
|--------|-------------|
| `result.has_trades()` | Any trades occurred? |
| `result.is_resting()` | Order is on the book? |
| `result.is_fully_filled()` | Entire quantity filled? |

#### `Trade`

```rust
pub struct Trade {
    pub id: TradeId,
    pub price: Price,                // Resting order's price
    pub quantity: Quantity,           // Quantity executed
    pub aggressor_order_id: OrderId,  // Taker order ID
    pub passive_order_id: OrderId,    // Maker order ID
    pub aggressor_side: Side,         // Buy or Sell
    pub timestamp: Timestamp,
}
```

| Method | Description |
|--------|-------------|
| `trade.passive_side()` | Opposite of `aggressor_side` |
| `trade.notional()` | `price.0 * quantity as i64` |

Trades always execute at the **resting order's price** — the aggressor gets price improvement.

#### `CancelResult`

```rust
pub struct CancelResult {
    pub success: bool,
    pub cancelled_quantity: Quantity,
    pub error: Option<CancelError>,
}
```

```rust
pub enum CancelError {
    OrderNotFound,
    OrderNotActive,
}
```

#### `ModifyResult`

```rust
pub struct ModifyResult {
    pub success: bool,
    pub old_order_id: OrderId,
    pub new_order_id: Option<OrderId>,   // Set on success
    pub cancelled_quantity: Quantity,
    pub trades: Vec<Trade>,              // From the replacement order
    pub error: Option<ModifyError>,
}
```

```rust
pub enum ModifyError {
    OrderNotFound,
    OrderNotActive,
    InvalidQuantity,   // new_quantity == 0
}
```

#### `Order`

```rust
pub struct Order {
    pub id: OrderId,
    pub side: Side,
    pub price: Price,
    pub original_quantity: Quantity,
    pub remaining_quantity: Quantity,
    pub filled_quantity: Quantity,
    pub timestamp: Timestamp,
    pub time_in_force: TimeInForce,
    pub status: OrderStatus,
}
```

| Method | Description |
|--------|-------------|
| `order.is_active()` | Status is `New` or `PartiallyFilled` |

---

### Snapshots

#### `BookSnapshot`

A point-in-time view of the order book.

```rust
pub struct BookSnapshot {
    pub bids: Vec<LevelSnapshot>,  // Highest price first
    pub asks: Vec<LevelSnapshot>,  // Lowest price first
    pub timestamp: Timestamp,
}
```

| Method | Returns |
|--------|---------|
| `snap.best_bid()` | `Option<Price>` |
| `snap.best_ask()` | `Option<Price>` |
| `snap.spread()` | `Option<i64>` — best ask minus best bid |
| `snap.mid_price()` | `Option<f64>` — (bid + ask) / 2 |
| `snap.total_bid_quantity()` | `Quantity` — sum across all bid levels |
| `snap.total_ask_quantity()` | `Quantity` — sum across all ask levels |

#### `LevelSnapshot`

A single price level in the snapshot.

```rust
pub struct LevelSnapshot {
    pub price: Price,
    pub quantity: Quantity,      // Aggregate quantity at this level
    pub order_count: usize,     // Number of orders at this level
}
```

#### Snapshot Depths

```rust
// L1: best bid/ask only
let (bid, ask) = exchange.best_bid_ask();

// L2: top N levels — most common for strategies
let snap = exchange.depth(5);
for level in &snap.bids {
    println!("{}: {} shares ({} orders)", level.price, level.quantity, level.order_count);
}

// L3: full book — all levels, all detail
let full = exchange.full_book();
```

---

### Advanced Types

These types are re-exported for users who need low-level access to the book internals. Most users only need `Exchange`.

#### `OrderBook`

The underlying order book. Accessed via `exchange.book()`.

```rust
let book: &OrderBook = exchange.book();

// Direct queries
let bid: Option<Price> = book.best_bid();
let count: usize = book.order_count();        // total orders (active + terminal)
let active: usize = book.active_order_count(); // only active orders
let crossed: bool = book.is_crossed();         // best bid >= best ask?
```

#### `PriceLevels`

One side of the order book (all bids or all asks). Sorted by price, best-to-worst.

```rust
let bids: &PriceLevels = book.bids();
let asks: &PriceLevels = book.asks();

let levels: usize = bids.level_count();          // number of distinct price levels
let total: Quantity = bids.total_quantity();       // sum of all quantities
let at_price: Quantity = asks.quantity_at_or_better(Price(101_00));

// Iterate best to worst
for (price, level) in bids.iter_best_to_worst() {
    println!("{}: {} shares, {} orders", price, level.total_quantity(), level.order_count());
}
```

#### `Level`

A single price level — a FIFO queue of orders.

```rust
let level: &Level = bids.best_level().unwrap();

let price: Price = level.price();
let qty: Quantity = level.total_quantity();
let count: usize = level.order_count();
let front: Option<OrderId> = level.front();  // first in queue
```

#### `MatchResult`

Returned by `OrderBook::match_order` (low-level matching). Most users get trades via `SubmitResult` instead.

```rust
pub struct MatchResult {
    pub trades: Vec<Trade>,
    pub remaining_quantity: Quantity,
}
```

| Method | Description |
|--------|-------------|
| `result.filled_quantity()` | Total quantity matched |
| `result.is_fully_filled()` | No remaining quantity? |
| `result.is_empty()` | No trades occurred? |

---

### Event Replay

**Feature flag:** `event-log` (enabled by default)

The exchange records every input as an `Event`. Replaying the same events on a fresh exchange produces identical state — same order IDs, same trades, same book.

#### Event Types

```rust
pub enum Event {
    SubmitLimit { side, price, quantity, time_in_force },
    SubmitMarket { side, quantity },
    Cancel { order_id },
    Modify { order_id, new_price, new_quantity },
}
```

Convenience constructors:

```rust
use nanobook::{Event, Side, Price, OrderId, TimeInForce};

let e1 = Event::submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
let e2 = Event::submit_market(Side::Sell, 50);
let e3 = Event::cancel(OrderId(1));
let e4 = Event::modify(OrderId(2), Price(99_00), 200);
```

#### Applying Events

```rust
pub fn apply(&mut self, event: &Event) -> ApplyResult
pub fn apply_all(&mut self, events: &[Event]) -> Vec<Trade>
pub fn replay(events: &[Event]) -> Exchange
```

`ApplyResult` contains:

```rust
pub struct ApplyResult {
    pub trades: Vec<Trade>,
}
```

#### Replay Example

```rust
use nanobook::{Exchange, Event, Side, Price, TimeInForce};

// Original session
let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
exchange.submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
exchange.submit_limit(Side::Buy, Price(101_00), 75, TimeInForce::GTC);

// Save events (serialize these for persistence)
let events = exchange.events().to_vec();

// Reconstruct identical state
let replayed = Exchange::replay(&events);
assert_eq!(exchange.best_bid_ask(), replayed.best_bid_ask());
assert_eq!(exchange.trades().len(), replayed.trades().len());
```

#### Managing the Event Log

```rust
// Access recorded events
let events: &[Event] = exchange.events();

// Clear after persisting (state is preserved)
exchange.clear_events();
```

#### Disabling Event Logging

For maximum performance, disable the feature:

```toml
[dependencies]
nanobook = { version = "0.4", default-features = false }
```

This removes `apply`, `apply_all`, `replay`, `events`, and `clear_events` from the API.

---

## Symbol

A fixed-size instrument identifier. Stored as `[u8; 8]` inline — `Copy`, no heap allocation, max 8 ASCII bytes.

```rust
use nanobook::Symbol;

let sym = Symbol::new("AAPL");
assert_eq!(sym.as_str(), "AAPL");
assert_eq!(format!("{sym}"), "AAPL");

// Try fallible construction
assert!(Symbol::try_new("TOOLONGNAME").is_none());

// Works as HashMap key (implements Hash, Eq, Ord)
use std::collections::HashMap;
let mut map = HashMap::new();
map.insert(Symbol::new("MSFT"), 42);
```

Implements: `Clone`, `Copy`, `PartialEq`, `Eq`, `Hash`, `PartialOrd`, `Ord`, `Display`, `Debug`, `AsRef<str>`.

With `serde` feature: serializes as a string.

---

## MultiExchange

A collection of per-symbol `Exchange` instances. Each symbol gets an independent order book.

```rust
use nanobook::{MultiExchange, Symbol, Side, Price, TimeInForce};

let mut multi = MultiExchange::new();
let aapl = Symbol::new("AAPL");
let msft = Symbol::new("MSFT");

// Orders route to per-symbol books
multi.get_or_create(&aapl).submit_limit(Side::Sell, Price(150_00), 100, TimeInForce::GTC);
multi.get_or_create(&msft).submit_limit(Side::Sell, Price(300_00), 200, TimeInForce::GTC);

// Query
assert_eq!(multi.get(&aapl).unwrap().best_ask(), Some(Price(150_00)));
assert!(multi.get(&Symbol::new("GOOG")).is_none());

// Iterate all symbols
for (sym, bid, ask) in multi.best_prices() {
    println!("{sym}: bid={bid:?} ask={ask:?}");
}
```

| Method | Description |
|--------|-------------|
| `get_or_create(&Symbol)` | Get or create exchange for symbol |
| `get(&Symbol)` | Read-only access (returns `Option`) |
| `get_mut(&Symbol)` | Mutable access (returns `Option`) |
| `symbols()` | Iterator over all symbols |
| `best_prices()` | Vec of `(Symbol, Option<Price>, Option<Price>)` |
| `len()` / `is_empty()` | Number of symbols |

---

## Portfolio Engine

**Feature flag:** `portfolio`

```toml
[dependencies]
nanobook = { version = "0.4", features = ["portfolio"] }
```

### Portfolio

Tracks cash, positions, costs, returns, and equity over time.

```rust
use nanobook::portfolio::{Portfolio, CostModel};
use nanobook::Symbol;

let cost = CostModel { commission_bps: 5, slippage_bps: 3, min_trade_fee: 1_00 };
let mut portfolio = Portfolio::new(1_000_000_00, cost); // $1,000,000

let aapl = Symbol::new("AAPL");
let prices = [(aapl, 150_00)];

// Rebalance to 60% AAPL
portfolio.rebalance_simple(&[(aapl, 0.6)], &prices);

// Record period return
portfolio.record_return(&prices);

// Query state
let equity = portfolio.total_equity(&prices);
let weights = portfolio.current_weights(&prices);
let snap = portfolio.snapshot(&prices);
```

#### Execution Modes

**SimpleFill** — instant execution at bar prices. Fast, no microstructure:

```rust
portfolio.rebalance_simple(&[(aapl, 0.6)], &prices);
```

**LOBFill** — route through real `Exchange` matching engines:

```rust
use nanobook::MultiExchange;

let mut exchanges = MultiExchange::new();
// ... populate LOBs with orders ...
portfolio.rebalance_lob(&[(aapl, 0.6)], &mut exchanges);
```

| Method | Description |
|--------|-------------|
| `new(cash, cost_model)` | Create portfolio with initial cash |
| `rebalance_simple(targets, prices)` | Rebalance at given prices |
| `rebalance_lob(targets, exchanges)` | Rebalance through LOB |
| `record_return(prices)` | Record a period return |
| `total_equity(prices)` | Cash + position values |
| `current_weights(prices)` | Position weights as fractions |
| `snapshot(prices)` | Point-in-time snapshot |
| `cash()` | Current cash balance |
| `position(symbol)` | Get position by symbol |
| `positions()` | Iterator over all positions |
| `returns()` | Accumulated return series |
| `equity_curve()` | Equity at each snapshot |

### Position

Per-symbol position tracking with VWAP entry price and realized PnL.

```rust
use nanobook::portfolio::Position;
use nanobook::Symbol;

let mut pos = Position::new(Symbol::new("AAPL"));

pos.apply_fill(100, 150_00);   // buy 100 @ $150
pos.apply_fill(-50, 160_00);   // sell 50 @ $160

assert_eq!(pos.quantity, 50);
assert_eq!(pos.avg_entry_price, 150_00);
assert_eq!(pos.realized_pnl, 50 * 10_00);  // $500 profit
assert_eq!(pos.unrealized_pnl(155_00), 50 * 5_00); // $250
```

### CostModel

```rust
use nanobook::portfolio::CostModel;

let model = CostModel {
    commission_bps: 10,  // 0.10%
    slippage_bps: 5,     // 0.05%
    min_trade_fee: 1_00, // $1.00 minimum
};

let cost = model.compute_cost(1_000_000); // 15 bps on $10,000 = $15
assert_eq!(cost, 1500);

let zero = CostModel::zero(); // no fees
```

### Financial Metrics

```rust
use nanobook::portfolio::compute_metrics;

let returns = vec![0.01, -0.005, 0.02, 0.015, -0.01, 0.008];
let metrics = compute_metrics(&returns, 252.0, 0.04/252.0).unwrap();

println!("{metrics}"); // Formatted output:
// Performance Metrics
//   Total return:       3.82%
//   CAGR:              ...
//   Sharpe:            ...
//   Max drawdown:      ...
```

| Field | Description |
|-------|-------------|
| `total_return` | Cumulative return (e.g., 0.15 = 15%) |
| `cagr` | Compound annual growth rate |
| `volatility` | Annualized standard deviation |
| `sharpe` | Annualized Sharpe ratio |
| `sortino` | Annualized Sortino ratio |
| `max_drawdown` | Peak-to-trough (e.g., 0.20 = 20%) |
| `calmar` | CAGR / max_drawdown |
| `num_periods` | Number of return periods |
| `winning_periods` | Periods with positive return |
| `losing_periods` | Periods with negative return |

### Parallel Sweep

**Feature flag:** `parallel` (implies `portfolio`)

```toml
[dependencies]
nanobook = { version = "0.4", features = ["parallel"] }
```

Run strategy variants in parallel using rayon:

```rust
use nanobook::portfolio::sweep::sweep;

let params = vec![0.5_f64, 1.0, 1.5, 2.0]; // e.g., leverage levels
let results = sweep(&params, 12.0, 0.0, |&leverage| {
    // Each invocation creates its own portfolio
    vec![0.01 * leverage, -0.005 * leverage, 0.02 * leverage]
});

for (i, metrics) in results.iter().enumerate() {
    if let Some(m) = metrics {
        println!("Leverage {:.1}: Sharpe={:.2}", params[i], m.sharpe);
    }
}
```

---

## Book Analytics

### Order Book Imbalance

```rust
let snap = exchange.depth(10);
if let Some(imb) = snap.imbalance() {
    // Range [-1.0, 1.0]: positive = buy pressure, negative = sell pressure
    println!("Imbalance: {imb:.4}");
}
```

### Weighted Midpoint

```rust
if let Some(wmid) = snap.weighted_mid() {
    // Leans toward the side with less liquidity
    println!("Weighted mid: {wmid:.2}");
}
```

### VWAP

```rust
use nanobook::Trade;

let trades = exchange.trades();
if let Some(vwap) = Trade::vwap(trades) {
    println!("VWAP: {vwap}");
}
```

---

## Time-in-Force Semantics

### GTC (Good-Til-Cancelled)

The default. Order rests on the book until fully filled or explicitly cancelled. Partial fills are allowed.

```rust
let mut exchange = Exchange::new();

// Resting ask at $101
exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);

// Buy 50 at $101 — partial fill, remainder rests
let result = exchange.submit_limit(Side::Buy, Price(101_00), 150, TimeInForce::GTC);
assert_eq!(result.filled_quantity, 100);
assert_eq!(result.resting_quantity, 50);   // 50 rests on the bid side
assert_eq!(result.status, OrderStatus::PartiallyFilled);
```

### IOC (Immediate-or-Cancel)

Fill whatever is available immediately, cancel the rest. Never rests on book.

```rust
let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(100_00), 30, TimeInForce::GTC);

let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);
assert_eq!(result.filled_quantity, 30);
assert_eq!(result.cancelled_quantity, 70);  // remainder cancelled
assert_eq!(result.resting_quantity, 0);     // IOC never rests
assert_eq!(exchange.best_bid(), None);      // nothing on the book
```

No liquidity at all? The entire order is cancelled:

```rust
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::IOC);
assert_eq!(result.status, OrderStatus::Cancelled);
assert_eq!(result.cancelled_quantity, 100);
```

### FOK (Fill-or-Kill)

All-or-nothing. If the full quantity cannot be filled immediately, the **entire order is rejected** — no trades occur, no book state changes.

```rust
let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);

// Try to buy 100 but only 50 available — rejected entirely
let result = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::FOK);
assert_eq!(result.status, OrderStatus::Cancelled);
assert_eq!(result.filled_quantity, 0);       // no fills
assert_eq!(result.cancelled_quantity, 100);  // entire order cancelled
assert!(result.trades.is_empty());           // no trades
assert_eq!(exchange.best_ask(), Some(Price(100_00)));  // book unchanged
```

### Summary Table

| TIF | Partial Fill? | Rests on Book? | No Liquidity |
|-----|:---:|:---:|---|
| **GTC** | Yes | Yes (remainder) | Rests entirely |
| **IOC** | Yes | No (remainder cancelled) | Cancelled |
| **FOK** | No | No (all-or-nothing) | Cancelled |

---

## CLI Reference

An interactive REPL for experimenting with the order book.

```bash
cargo run --bin lob
```

### Commands

| Command | Description | Example |
|---------|-------------|---------|
| `buy <price> <qty> [ioc\|fok]` | Submit buy limit (default: GTC) | `buy 100.50 100` |
| `sell <price> <qty> [ioc\|fok]` | Submit sell limit (default: GTC) | `sell 101.00 50 ioc` |
| `market <buy\|sell> <qty>` | Submit market order | `market buy 200` |
| `stop <buy\|sell> <price> <qty>` | Submit stop-market order | `stop buy 105.00 100` |
| `stoplimit <buy\|sell> <stop> <limit> <qty> [ioc\|fok]` | Submit stop-limit order | `stoplimit sell 95 94.50 100` |
| `cancel <order_id>` | Cancel an order (regular or stop) | `cancel 3` |
| `status <order_id>` | Show order details | `status 1` |
| `book` or `b` | Display order book | `book` |
| `trades` or `t` | Show trade history (last 20) | `trades` |
| `save <path>` | Save exchange state (persistence feature) | `save orders.jsonl` |
| `load <path>` | Load exchange state (persistence feature) | `load orders.jsonl` |
| `clear` | Reset the exchange | `clear` |
| `help` or `h` | Show help | `help` |
| `quit` or `exit` | Exit | `quit` |

Prices are entered in **dollars** (e.g., `100.50` = $100.50), automatically converted to cents internally.

### Example Session

```
lob> sell 101.00 100
Order #1: SELL 100 @ $101.00 GTC
  Status: New (filled: 0, resting: 100, cancelled: 0)

lob> sell 100.50 50
Order #2: SELL 50 @ $100.50 GTC
  Status: New (filled: 0, resting: 50, cancelled: 0)

lob> buy 101.00 120
Order #3: BUY 120 @ $101.00 GTC
  Trades:
    50 @ $100.50
    70 @ $101.00
  Status: Filled (filled: 120, resting: 0, cancelled: 0)

lob> book
            ORDER BOOK
  ──────────────────────────────
  ASK $  101.00      30  (1 orders)
  ─────── spread: $1.00 ───────
  (no bids)
```

---

## Performance

### Benchmarks

Measured single-threaded on AMD Ryzen / Intel Core:

| Operation | Latency | Throughput | Complexity |
|-----------|---------|------------|------------|
| Submit (no match) | **120 ns** | 8.3M ops/sec | O(log P) |
| Submit (with match) | ~200 ns | 5M ops/sec | O(log P + M) |
| BBO query | **1 ns** | 1B ops/sec | O(1) |
| Cancel | 660 ns | 1.5M ops/sec | O(N) |
| L2 snapshot (10 levels) | ~500 ns | 2M ops/sec | O(D) |

Where: P = price levels, M = orders matched, N = orders at price level, D = depth requested.

### Time Breakdown (Submit, No Match)

```
submit_limit() ~120 ns:
├── FxHashMap insert     ~30 ns   order storage
├── BTreeMap insert      ~30 ns   price level (O(log P))
├── VecDeque push         ~5 ns   FIFO queue
├── Event recording      ~10 ns   (optional, for replay)
└── Overhead             ~45 ns   struct creation, etc.
```

### Run Benchmarks

```bash
cargo bench
```

### Optimizations Applied

1. **FxHash** — Non-cryptographic hash for OrderId lookups (+25% vs std HashMap)
2. **Cached BBO** — Best bid/ask cached for O(1) access
3. **Optional event logging** — Disable for max throughput:

```bash
# With event logging (default)
cargo build --release

# Without event logging — maximum performance
cargo build --release --no-default-features
```

---

## Input Validation

The `try_submit_*` methods validate inputs before processing:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce, ValidationError};

let mut exchange = Exchange::new();

// Zero quantity
let err = exchange.try_submit_limit(Side::Buy, Price(100_00), 0, TimeInForce::GTC);
assert_eq!(err.unwrap_err(), ValidationError::ZeroQuantity);

// Zero/negative price
let err = exchange.try_submit_limit(Side::Buy, Price(0), 100, TimeInForce::GTC);
assert_eq!(err.unwrap_err(), ValidationError::ZeroPrice);

// Valid
let ok = exchange.try_submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
assert!(ok.is_ok());

// Market orders only check quantity
let err = exchange.try_submit_market(Side::Buy, 0);
assert_eq!(err.unwrap_err(), ValidationError::ZeroQuantity);
```

The original `submit_limit` and `submit_market` accept any input (no breaking change).

---

## Stop Orders

Stop orders wait until the last trade price reaches a threshold, then trigger as regular orders.

### Stop-Market

```rust
use nanobook::{Exchange, Side, Price, TimeInForce, StopStatus};

let mut exchange = Exchange::new();

// Place a buy stop: triggers when last_trade_price >= $105
let stop = exchange.submit_stop_market(Side::Buy, Price(105_00), 100);
assert_eq!(stop.status, StopStatus::Pending);

// When a trade occurs at $105+, the stop triggers as a market buy for 100 shares
```

### Stop-Limit

```rust
// Triggers at $105, then submits a limit buy at $106
let stop = exchange.submit_stop_limit(
    Side::Buy,
    Price(105_00),   // stop price
    Price(106_00),   // limit price
    100,
    TimeInForce::GTC,
);
```

### Trigger Rules

| Side | Triggers When |
|------|---------------|
| Buy stop | `last_trade_price >= stop_price` |
| Sell stop | `last_trade_price <= stop_price` |

### Key Behaviors

- **Immediate trigger**: If `last_trade_price` already past stop price, triggers on submission
- **Cascade**: Triggered stops may produce trades that trigger more stops (max 100 iterations)
- **Cancel**: `exchange.cancel(stop_id)` works on both regular and stop orders
- **Modify**: Not supported for stops — cancel and re-submit instead
- **ID space**: Stop orders share the global OrderId space

### Queries

```rust
exchange.get_stop_order(order_id);   // Option<&StopOrder>
exchange.pending_stop_count();        // usize
exchange.last_trade_price();          // Option<Price>
exchange.stop_book();                 // &StopBook
```

---

## Trailing Stops

Trailing stops automatically adjust their trigger price as the market moves favorably. Three trailing methods are supported.

### Fixed Trailing

The stop price trails by a fixed offset in cents:

```rust
use nanobook::{Exchange, Side, Price, TrailMethod};

let mut exchange = Exchange::new();

// Sell trailing stop: triggers if price drops $2.00 from peak
let stop = exchange.submit_trailing_stop_market(
    Side::Sell,
    Price(98_00),       // initial stop price
    100,                // quantity
    TrailMethod::Fixed(200), // trail by $2.00
);
```

### Percentage Trailing

The stop price trails by a percentage of the watermark:

```rust
// Trail by 5% from the peak
let stop = exchange.submit_trailing_stop_market(
    Side::Sell,
    Price(95_00),
    100,
    TrailMethod::Percentage(0.05),
);
```

### ATR-Based Trailing

Adaptive trailing using Average True Range computed from trade prices:

```rust
// Trail by 2x ATR over 14-period window
let stop = exchange.submit_trailing_stop_market(
    Side::Sell,
    Price(95_00),
    100,
    TrailMethod::Atr { multiplier: 2.0, period: 14 },
);
```

ATR is computed internally from absolute price changes between consecutive trades (adapted for tick-level data where OHLC bars are not available).

### Trailing Stop-Limit

```rust
use nanobook::TimeInForce;

// Trailing stop that triggers a limit order instead of a market order
let stop = exchange.submit_trailing_stop_limit(
    Side::Sell,
    Price(95_00),       // initial stop price
    Price(94_50),       // limit price
    100,
    TimeInForce::GTC,
    TrailMethod::Fixed(200),
);
```

### Behavior

- **Sell trailing**: watermark tracks market highs; stop price = watermark - offset
- **Buy trailing**: watermark tracks market lows; stop price = watermark + offset
- Stop price only moves in the favorable direction (never worsens)
- Watermark and stop price update automatically when trades occur
- Cancel with `exchange.cancel(stop_id)` like any other stop order

---

## Strategy Trait

**Feature flag:** `portfolio`

The `Strategy` trait provides a batch-oriented interface for backtesting. Implement `compute_weights()` and use `run_backtest()` to run the full loop.

### Implementing a Strategy

```rust
use nanobook::portfolio::strategy::{Strategy, run_backtest, BacktestResult};
use nanobook::portfolio::{Portfolio, CostModel};
use nanobook::Symbol;

struct MomentumStrategy {
    lookback: usize,
}

impl Strategy for MomentumStrategy {
    fn compute_weights(
        &self,
        bar_index: usize,
        prices: &[(Symbol, i64)],
        _portfolio: &Portfolio,
    ) -> Vec<(Symbol, f64)> {
        // Equal-weight all symbols after lookback period
        if bar_index < self.lookback {
            return vec![];
        }
        let w = 1.0 / prices.len() as f64;
        prices.iter().map(|(sym, _)| (*sym, w)).collect()
    }
}
```

### Running a Backtest

```rust
let strategy = MomentumStrategy { lookback: 3 };
let aapl = Symbol::new("AAPL");

let price_series = vec![
    vec![(aapl, 150_00)],
    vec![(aapl, 155_00)],
    vec![(aapl, 160_00)],
    vec![(aapl, 158_00)],
];

let result = run_backtest(
    &strategy,
    &price_series,
    1_000_000_00,       // initial cash
    CostModel::zero(),
    12.0,               // periods per year
    0.0,                // risk-free rate
);

if let Some(metrics) = &result.metrics {
    println!("{metrics}");
}
```

### Built-in EqualWeight

```rust
use nanobook::portfolio::strategy::EqualWeight;

let result = run_backtest(
    &EqualWeight,
    &price_series,
    1_000_000_00,
    CostModel::zero(),
    12.0,
    0.0,
);
```

### Parallel Strategy Sweep

**Feature flag:** `parallel`

```rust
use nanobook::portfolio::sweep::sweep_strategy;

let params = vec![1, 3, 5, 10, 20]; // lookback periods
let results = sweep_strategy(
    &params,
    &price_series,
    1_000_000_00,
    CostModel::zero(),
    12.0,
    0.0,
    |&lookback| MomentumStrategy { lookback },
);
```

---

## Persistence

**Feature flag:** `persistence` (includes `serde` and `event-log`)

```toml
[dependencies]
nanobook = { version = "0.4", features = ["persistence"] }
```

Save and load exchange state via JSON Lines event sourcing:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};
use std::path::Path;

let mut exchange = Exchange::new();
exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
exchange.submit_limit(Side::Buy, Price(100_00), 200, TimeInForce::GTC);

// Save to file (JSON Lines format)
exchange.save(Path::new("orders.jsonl")).unwrap();

// Load from file (replays all events)
let loaded = Exchange::load(Path::new("orders.jsonl")).unwrap();
assert_eq!(exchange.best_bid_ask(), loaded.best_bid_ask());
```

### Lower-Level API

```rust
use nanobook::persistence::{save_events, load_events};
use std::path::Path;

// Save/load event vectors directly
save_events(&events, Path::new("events.jsonl")).unwrap();
let events = load_events(Path::new("events.jsonl")).unwrap();
```

### Format

One JSON object per line (`.jsonl`). Human-readable, streamable:

```jsonl
{"SubmitLimit":{"side":"Sell","price":10100,"quantity":100,"time_in_force":"GTC"}}
{"SubmitLimit":{"side":"Buy","price":10000,"quantity":200,"time_in_force":"GTC"}}
```

---

## Serde Support

**Feature flag:** `serde`

```toml
[dependencies]
nanobook = { version = "0.4", features = ["serde"] }
```

All public types derive `Serialize` and `Deserialize` when the `serde` feature is enabled:

`Price`, `OrderId`, `TradeId`, `Symbol`, `Side`, `TimeInForce`, `OrderStatus`, `Order`, `Trade`,
`Event`, `ApplyResult`, `SubmitResult`, `CancelResult`, `CancelError`, `ModifyResult`,
`ModifyError`, `BookSnapshot`, `LevelSnapshot`, `MatchResult`, `StopOrder`, `StopStatus`,
`StopSubmitResult`, `ValidationError`, `Position`, `CostModel`.

### Portfolio Persistence

**Feature flag:** `persistence`

Save and load portfolio state as JSON:

```rust
use nanobook::portfolio::{Portfolio, CostModel};
use std::path::Path;

let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
// ... rebalance, record returns ...

portfolio.save_json(Path::new("portfolio.json")).unwrap();
let loaded = Portfolio::load_json(Path::new("portfolio.json")).unwrap();
assert_eq!(portfolio.cash, loaded.cash);
```

---

## Python Bindings

Install via maturin (from the `python/` directory):

```bash
cd python && maturin develop --release
```

### Exchange

```python
import nanobook

ex = nanobook.Exchange()

# Limit orders — strings for side/tif, ints for price (cents) and quantity
result = ex.submit_limit("buy", 10050, 100, "gtc")
print(result.order_id, result.status)  # 1 New

# Market orders
result = ex.submit_market("sell", 50)

# Cancel and modify
ex.cancel(result.order_id)
ex.modify(order_id, 10100, 200)

# Queries
bid, ask = ex.best_bid_ask()
spread = ex.spread()
snap = ex.depth(10)
trades = ex.trades()
```

### Stop Orders

```python
# Stop-market
ex.submit_stop_market("sell", 9500, 100)

# Stop-limit
ex.submit_stop_limit("buy", 10500, 10600, 100, "gtc")

# Trailing stops
ex.submit_trailing_stop_market("sell", 9500, 100, "fixed", 200)
ex.submit_trailing_stop_market("sell", 9500, 100, "percentage", 0.05)
ex.submit_trailing_stop_market("sell", 9500, 100, "atr", 2.0, atr_period=14)
```

### Portfolio

```python
cost = nanobook.CostModel(commission_bps=10, slippage_bps=5)
p = nanobook.Portfolio(1_000_000_00, cost)

# Rebalance to target weights
p.rebalance_simple([("AAPL", 0.6)], [("AAPL", 150_00)])

# Record returns and compute metrics
p.record_return([("AAPL", 155_00)])
m = p.compute_metrics(12.0, 0.0)
print(f"Sharpe: {m.sharpe:.2f}, Total return: {m.total_return:.2%}")

# Save/load (requires persistence feature)
p.save_json("portfolio.json")
loaded = nanobook.Portfolio.load_json("portfolio.json")
```

### Parallel Sweep

```python
# GIL released for Rayon parallelism
results = nanobook.py_sweep_equal_weight(
    n_params=100,
    price_series=[
        [("AAPL", 150_00), ("MSFT", 300_00)],
        [("AAPL", 155_00), ("MSFT", 310_00)],
    ],
    initial_cash=1_000_000_00,
    periods_per_year=252.0,
    risk_free=0.0,
)
for m in results:
    if m is not None:
        print(f"Sharpe={m.sharpe:.2f}")
```

### Standalone Metrics

```python
m = nanobook.py_compute_metrics([0.01, -0.005, 0.02], 252.0, 0.0)
print(f"Total return: {m.total_return:.2%}, Sharpe: {m.sharpe:.2f}")
```

---

## Common Patterns

### Strategy Backtesting

Feed historical events through the exchange and react to results:

```rust
use nanobook::{Exchange, Event, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

let historical_events = vec![
    Event::submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC),
    Event::submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC),
    Event::submit_market(Side::Buy, 50),
];

for event in &historical_events {
    let result = exchange.apply(event);
    if !result.trades.is_empty() {
        let (bid, ask) = exchange.best_bid_ask();
        // React to trades and updated book state...
    }
}
```

### Market Impact Analysis

Measure how a large order moves the market:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

// Build up some liquidity
for i in 0..10 {
    exchange.submit_limit(Side::Sell, Price(100_00 + i * 10), 100, TimeInForce::GTC);
}

let ask_before = exchange.best_ask().unwrap();
let result = exchange.submit_market(Side::Buy, 500);
let ask_after = exchange.best_ask();

let impact = match ask_after {
    Some(ask) => ask.0 - ask_before.0,
    None => -1, // exhausted all liquidity
};
println!("Filled {} shares, market impact: {} ticks", result.filled_quantity, impact);
```

### Queue Position Testing

Verify FIFO priority at the same price level:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

// Two orders at the same price — first one has priority
let first = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);
let second = exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC);

// Sell 100 — fills the first order (FIFO)
exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);

let first_order = exchange.get_order(first.order_id).unwrap();
let second_order = exchange.get_order(second.order_id).unwrap();
assert_eq!(first_order.filled_quantity, 100);   // filled
assert_eq!(second_order.filled_quantity, 0);    // still waiting
```

### IOC for Aggressive Execution

Take available liquidity without leaving a resting order:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

// Some liquidity on the ask side
exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
exchange.submit_limit(Side::Sell, Price(100_50), 80, TimeInForce::GTC);

// Sweep up to $100.50, cancel unfilled remainder
let result = exchange.submit_limit(Side::Buy, Price(100_50), 200, TimeInForce::IOC);
assert_eq!(result.filled_quantity, 130);     // 50 + 80
assert_eq!(result.cancelled_quantity, 70);   // 200 - 130
assert_eq!(exchange.best_bid(), None);       // nothing resting
```

### Checkpoint and Replay

Save exchange state for later reconstruction:

```rust
use nanobook::{Exchange, Side, Price, TimeInForce};

let mut exchange = Exchange::new();

// Normal operations...
exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
exchange.submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC);
exchange.submit_market(Side::Buy, 50);

// Checkpoint: save events (you'd serialize these to disk/network)
let checkpoint = exchange.events().to_vec();

// Later: reconstruct exact state
let restored = Exchange::replay(&checkpoint);
assert_eq!(exchange.best_bid_ask(), restored.best_bid_ask());
assert_eq!(exchange.trades().len(), restored.trades().len());

// Continue from checkpoint
exchange.clear_events();  // free memory, state preserved
```

---

## Limitations

nanobook is an **educational and testing tool**, not a production exchange:

| Limitation | Description |
|------------|-------------|
| **No networking** | In-process only; no TCP/UDP/WebSocket server |
| **No compliance** | No self-trade prevention, circuit breakers, or regulatory controls |
| **No complex orders** | No iceberg or pegged orders |
| **Single-threaded** | No concurrent access; wrap in `Mutex` if needed |
