# SPECS.md — Technical Specification

## Overview

| Property | Value |
|----------|-------|
| **Name** | rustbook |
| **Purpose** | Deterministic exchange simulator for strategy testing |
| **Language** | Rust (2021 edition) |
| **Scope** | Educational/testing quality (not production-grade) |

---

## 1. Core Concepts

### 1.1 Order Book Model

The order book is a **continuous double auction** with **price-time priority**:

1. **Price priority**: Better prices execute first
   - Bids: Higher price = better
   - Asks: Lower price = better

2. **Time priority**: At same price, earlier orders execute first (FIFO)

3. **Trade price**: Executes at the **resting order's price** (price improvement for aggressor)

### 1.2 Price Representation

Prices are **fixed-point integers** to avoid floating-point errors:

```rust
/// Price in smallest units (e.g., cents, basis points)
/// Price(10050) = $100.50 if tick size is $0.01
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Price(pub i64);

impl Price {
    pub const ZERO: Price = Price(0);
    pub const MAX: Price = Price(i64::MAX);
    pub const MIN: Price = Price(i64::MIN);
}
```

**Rationale**: Financial systems require exact arithmetic. `100.10 + 100.20` must equal `200.30`, not `200.30000000000001`.

### 1.3 Quantity Representation

Quantities are **unsigned 64-bit integers**:

```rust
pub type Quantity = u64;
```

- Minimum quantity is 1
- Zero quantity orders are rejected
- No fractional shares

### 1.4 Timestamp Representation

Timestamps are **nanoseconds since exchange start**:

```rust
pub type Timestamp = u64;
```

- Monotonically increasing (never decreases or repeats)
- Assigned by exchange on order receipt
- NOT system clock (ensures determinism)
- Starts at 1, not 0

### 1.5 Identifiers

```rust
/// Unique order identifier, assigned by exchange
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct OrderId(pub u64);

/// Unique trade identifier, assigned by exchange
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TradeId(pub u64);
```

Both start at 1 and increment monotonically.

---

## 2. Order Types

### 2.1 Order Side

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}
```

### 2.2 Time-in-Force

Controls how long an order remains active and how partial fills are handled:

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TimeInForce {
    /// Good-til-cancelled: rests on book until filled or explicitly cancelled
    #[default]
    GTC,
    /// Immediate-or-cancel: fill what's available immediately, cancel remainder
    IOC,
    /// Fill-or-kill: fill entire quantity immediately or cancel entire order
    FOK,
}
```

| TIF | Allows Partial Fill? | Rests on Book? | Use Case |
|-----|---------------------|----------------|----------|
| GTC | Yes | Yes | Passive liquidity provision |
| IOC | Yes | No | Aggressive execution without resting |
| FOK | No | No | All-or-nothing execution |

### 2.3 Limit Order

Specifies maximum (buy) or minimum (sell) price.

```rust
// Submitted via:
exchange.submit_limit(side: Side, price: Price, quantity: Quantity, tif: TimeInForce)
```

**Behavior**:
1. Check for immediate matches against opposite side
2. Execute any matches at resting order's price
3. Handle remainder based on TIF:
   - **GTC**: Rest on book at specified price
   - **IOC**: Cancel remainder
   - **FOK**: If any remainder, cancel entire order (no trades)

### 2.4 Market Order

Executes immediately at best available prices. Always uses IOC semantics.

```rust
// Submitted via:
exchange.submit_market(side: Side, quantity: Quantity)
```

**Behavior**:
1. Match against best available prices until filled or book exhausted
2. Any unfilled quantity is cancelled (never rests)

**Note**: A market order is equivalent to a limit order with price = MAX (for buys) or MIN (for sells) and TIF = IOC.

### 2.5 Cancel Request

Remove a resting order from the book.

```rust
// Submitted via:
exchange.cancel(order_id: OrderId) -> CancelResult
```

**Behavior**:
1. If order exists and has remaining quantity, remove from book
2. Update order status to `Cancelled`
3. Return cancelled quantity
4. If order doesn't exist or already completed, return error

### 2.6 Modify Request (Cancel/Replace)

Atomically cancel existing order and submit new one.

```rust
// Submitted via:
exchange.modify(order_id: OrderId, new_price: Price, new_quantity: Quantity) -> ModifyResult
```

**Behavior**:
1. Cancel existing order (fail if doesn't exist or already completed)
2. Submit new limit order with new price/quantity (inherits original TIF)
3. **Loses time priority** — new timestamp assigned
4. Returns new order ID and any trades from the new order

**Note**: Modify is not atomic with respect to the market — another order could execute between cancel and new submission. This matches real exchange behavior.

---

## 3. Order Lifecycle

```
                    ┌─────────────┐
                    │   PENDING   │ (before exchange assigns timestamp)
                    └──────┬──────┘
                           │ submit()
                           ▼
                    ┌─────────────┐
          ┌─────────│     NEW     │─────────┬──────────┐
          │         └──────┬──────┘         │          │
          │                │                │          │
     cancel()         match()          match()    TIF reject
          │           (partial)         (full)    (FOK fails)
          │                │                │          │
          ▼                ▼                ▼          ▼
   ┌─────────────┐  ┌─────────────┐  ┌─────────────┐  │
   │  CANCELLED  │  │   PARTIAL   │  │   FILLED    │  │
   └─────────────┘  └──────┬──────┘  └─────────────┘  │
                           │                          │
                      match()/cancel()                │
                           │                          │
                           ▼                          │
                    ┌─────────────┐                   │
                    │   FILLED/   │◄──────────────────┘
                    │  CANCELLED  │
                    └─────────────┘
```

### Order Status

```rust
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrderStatus {
    /// Order accepted, resting on book (may have partial fills)
    New,
    /// Some quantity filled, remainder still on book
    PartiallyFilled,
    /// Fully executed, no longer on book
    Filled,
    /// Removed by user request or TIF rules, no longer on book
    Cancelled,
}
```

---

## 4. Matching Engine

### 4.1 Algorithm

```
MATCH(incoming_order) -> (Vec<Trade>, remaining_quantity):
    trades = []
    remaining = incoming_order.quantity

    WHILE remaining > 0:
        opposite_book = GET_OPPOSITE_BOOK(incoming_order.side)
        best_level = opposite_book.best_level()

        IF best_level IS NONE:
            BREAK  // No liquidity

        IF NOT PRICES_CROSS(incoming_order, best_level.price):
            BREAK  // No match at this price

        WHILE remaining > 0 AND best_level NOT EMPTY:
            resting_order = best_level.front()
            fill_qty = MIN(remaining, resting_order.remaining_quantity)

            trade = Trade {
                id: NEXT_TRADE_ID(),
                price: best_level.price,  // Resting order's price
                quantity: fill_qty,
                aggressor_order_id: incoming_order.id,
                passive_order_id: resting_order.id,
                aggressor_side: incoming_order.side,
                timestamp: NEXT_TIMESTAMP(),
            }
            trades.APPEND(trade)

            remaining -= fill_qty
            resting_order.remaining_quantity -= fill_qty
            resting_order.filled_quantity += fill_qty

            IF resting_order.remaining_quantity == 0:
                resting_order.status = Filled
                best_level.POP_FRONT()

        IF best_level IS EMPTY:
            opposite_book.REMOVE_LEVEL(best_level.price)

    RETURN (trades, remaining)
```

### 4.2 Price Crossing Rules

```rust
fn prices_cross(incoming_side: Side, incoming_price: Price, resting_price: Price) -> bool {
    match incoming_side {
        Side::Buy => incoming_price >= resting_price,
        Side::Sell => incoming_price <= resting_price,
    }
}
```

### 4.3 Trade Price Rule

Always the **resting order's price** (passive side). This provides price improvement to the aggressor.

```
Example:
- Ask resting at $100.00
- Buy limit comes in at $101.00
- Trade executes at $100.00 (buyer saves $1.00 vs their limit)
```

### 4.4 FOK Handling

FOK orders require special handling:

```
SUBMIT_LIMIT_FOK(order):
    // First, simulate the match without executing
    (simulated_trades, remaining) = SIMULATE_MATCH(order)

    IF remaining > 0:
        // Cannot fill entirely — reject
        RETURN SubmitResult {
            order_id: NEXT_ORDER_ID(),
            status: Cancelled,
            trades: [],
        }
    ELSE:
        // Can fill entirely — execute for real
        RETURN SUBMIT_LIMIT_GTC(order)  // Will fully fill
```

---

## 5. Data Structures

### 5.1 Order

```rust
#[derive(Clone, Debug)]
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

impl Order {
    /// Returns true if order is still active (can be cancelled or filled)
    pub fn is_active(&self) -> bool {
        matches!(self.status, OrderStatus::New | OrderStatus::PartiallyFilled)
    }
}
```

**Invariant**: `original_quantity == remaining_quantity + filled_quantity` (cancelled quantity tracked separately)

### 5.2 Trade

```rust
#[derive(Clone, Debug)]
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

### 5.3 Level (Queue at Single Price)

```rust
pub struct Level {
    price: Price,
    orders: VecDeque<OrderId>,   // FIFO queue of order IDs
    total_quantity: Quantity,    // Cached sum of remaining quantities
}

impl Level {
    pub fn is_empty(&self) -> bool {
        self.orders.is_empty()
    }

    pub fn total_quantity(&self) -> Quantity {
        self.total_quantity
    }
}
```

### 5.4 Price Levels (One Side of Book)

```rust
pub struct PriceLevels {
    levels: BTreeMap<Price, Level>,
    best_price: Option<Price>,  // Cached for O(1) access
    side: Side,                 // Determines "best" direction
}

impl PriceLevels {
    /// O(1) - cached
    pub fn best_price(&self) -> Option<Price> {
        self.best_price
    }

    /// O(1) - cached
    pub fn best_level(&self) -> Option<&Level> {
        self.best_price.and_then(|p| self.levels.get(&p))
    }
}
```

### 5.5 Order Book

```rust
pub struct OrderBook {
    bids: PriceLevels,                      // Sorted high → low
    asks: PriceLevels,                      // Sorted low → high
    orders: HashMap<OrderId, Order>,        // All orders (active + historical)
    next_order_id: OrderId,
    next_trade_id: TradeId,
    next_timestamp: Timestamp,
}
```

---

## 6. API

### 6.1 Exchange Interface

```rust
pub struct Exchange {
    book: OrderBook,
    trades: Vec<Trade>,     // Complete trade history
    events: Vec<Event>,     // Event log for replay
}

impl Exchange {
    /// Create new exchange
    pub fn new() -> Self;

    /// Submit a limit order
    pub fn submit_limit(
        &mut self,
        side: Side,
        price: Price,
        quantity: Quantity,
        tif: TimeInForce,
    ) -> SubmitResult;

    /// Submit a market order (always IOC semantics)
    pub fn submit_market(
        &mut self,
        side: Side,
        quantity: Quantity,
    ) -> SubmitResult;

    /// Cancel an order
    pub fn cancel(&mut self, order_id: OrderId) -> CancelResult;

    /// Modify an order (cancel + replace, loses time priority)
    pub fn modify(
        &mut self,
        order_id: OrderId,
        new_price: Price,
        new_quantity: Quantity,
    ) -> ModifyResult;

    /// Get order by ID (includes historical orders)
    pub fn get_order(&self, order_id: OrderId) -> Option<&Order>;

    /// Get best bid and ask prices
    pub fn best_bid_ask(&self) -> (Option<Price>, Option<Price>);

    /// Get top N levels each side
    pub fn depth(&self, levels: usize) -> BookSnapshot;

    /// Get full order book snapshot
    pub fn full_book(&self) -> BookSnapshot;

    /// Get all trades (complete history)
    pub fn trades(&self) -> &[Trade];

    /// Get event log for deterministic replay
    pub fn events(&self) -> &[Event];

    /// Replay events to reconstruct state
    pub fn replay(events: &[Event]) -> Self;

    /// Apply a single event (for replay)
    pub fn apply(&mut self, event: &Event) -> ApplyResult;
}
```

### 6.2 Result Types

```rust
pub struct SubmitResult {
    pub order_id: OrderId,
    pub status: OrderStatus,
    pub trades: Vec<Trade>,
}

pub struct CancelResult {
    pub success: bool,
    pub cancelled_quantity: Quantity,
    pub error: Option<CancelError>,
}

#[derive(Debug)]
pub enum CancelError {
    OrderNotFound,
    OrderAlreadyCompleted,
}

pub struct ModifyResult {
    pub success: bool,
    pub old_order_id: OrderId,
    pub new_order_id: Option<OrderId>,
    pub cancelled_quantity: Quantity,
    pub trades: Vec<Trade>,
    pub error: Option<ModifyError>,
}

#[derive(Debug)]
pub enum ModifyError {
    OrderNotFound,
    OrderAlreadyCompleted,
    InvalidQuantity,
    InvalidPrice,
}

/// Result of applying an event during replay
pub struct ApplyResult {
    pub trades: Vec<Trade>,
}
```

### 6.3 Book Snapshot

```rust
pub struct BookSnapshot {
    pub bids: Vec<LevelSnapshot>,
    pub asks: Vec<LevelSnapshot>,
    pub timestamp: Timestamp,
}

pub struct LevelSnapshot {
    pub price: Price,
    pub quantity: Quantity,     // Total quantity at this level
    pub order_count: usize,     // Number of orders
}
```

---

## 7. Event Log

For deterministic replay, all inputs are logged as events:

```rust
#[derive(Clone, Debug)]
pub enum Event {
    SubmitLimit {
        side: Side,
        price: Price,
        quantity: Quantity,
        time_in_force: TimeInForce,
    },
    SubmitMarket {
        side: Side,
        quantity: Quantity,
    },
    Cancel {
        order_id: OrderId,
    },
    Modify {
        order_id: OrderId,
        new_price: Price,
        new_quantity: Quantity,
    },
}
```

**Replay guarantee**: `Exchange::replay(exchange.events())` produces an identical exchange state.

```rust
// Save state
let events = exchange.events().to_vec();

// Later, reconstruct exact same state
let restored = Exchange::replay(&events);
assert_eq!(exchange.best_bid_ask(), restored.best_bid_ask());
```

---

## 8. Error Handling

### 8.1 Validation Errors

Orders are validated before processing:

```rust
#[derive(Debug)]
pub enum ValidationError {
    ZeroQuantity,
    InvalidPrice,  // e.g., negative price for a buy
}
```

### 8.2 Submission Errors

```rust
impl Exchange {
    pub fn try_submit_limit(...) -> Result<SubmitResult, ValidationError>;
    pub fn try_submit_market(...) -> Result<SubmitResult, ValidationError>;
}
```

The non-`try_` versions panic on validation errors (for convenience in trusted contexts).

---

## 9. Performance Requirements

| Operation | Complexity | Target Latency |
|-----------|------------|----------------|
| Submit (no match) | O(log P) | <1μs |
| Submit (with match) | O(log P + M) | <10μs |
| Cancel | O(1)* | <100ns |
| Modify | O(log P) | <1μs |
| Best bid/ask | O(1) | <50ns |
| Depth (N levels) | O(N) | <1μs |

Where:
- P = number of distinct price levels
- M = number of orders matched
- *Cancel is O(1) for lookup + O(n) for removal from VecDeque; use indexed removal for true O(1)

**Throughput target**: >1M orders/second (mixed workload, single-threaded)

---

## 10. Invariants

The following must **always** hold:

1. **Price ordering**: Best bid < best ask (no crossed book after matching)
2. **FIFO within level**: Orders at same price filled in timestamp order
3. **Quantity conservation**: For each order, `original == remaining + filled`
4. **Determinism**: Same event sequence → identical final state
5. **Monotonic IDs**: Order IDs and trade IDs always increase
6. **Monotonic timestamps**: Timestamps always increase
7. **Consistent totals**: Level total quantity == sum of member order remaining quantities
8. **No orphans**: Every OrderId referenced in a Level exists in the order index
9. **No ghosts**: Every active order in the index exists in exactly one Level

---

## 11. Out of Scope (v1)

The following are **explicitly not included** in v1:

| Feature | Reason |
|---------|--------|
| **Self-trade prevention** | Compliance feature, requires trader identity |
| **Circuit breakers** | Market-wide feature, not single-book |
| **Iceberg orders** | Display quantity ≠ actual quantity adds complexity |
| **Pegged orders** | Requires reference price feed |
| **Stop orders** | Requires trigger mechanism / market data |
| **Good-til-date** | Requires time management beyond monotonic counter |
| **Minimum quantity** | Edge case, rarely needed |
| **All-or-none (AON)** | Different from FOK (can wait for liquidity) |
| **Multi-symbol** | Single book only; use multiple Exchange instances |
| **Persistence** | In-memory only; serialize events for durability |
| **Networking** | In-process only; wrap in server for network access |

---

## 12. Testing Strategy

### 12.1 Unit Tests

- `Price` arithmetic and comparisons
- `Side::opposite()` correctness
- `Level` push/pop/peek operations
- `BTreeMap` ordering for bids (descending) vs asks (ascending)
- Validation error detection

### 12.2 Integration Tests

- Simple limit order → rests on book
- Market order → fills and cancels remainder
- Partial fills across multiple resting orders
- Cancel active order
- Cancel already-filled order (error)
- Modify order (loses priority)
- Multiple orders same price (FIFO preserved)
- Multiple orders different prices (price priority)
- IOC: fills available, cancels rest
- FOK: all-or-nothing behavior

### 12.3 Property-Based Tests

Using `proptest`:

```rust
proptest! {
    #[test]
    fn invariants_preserved(ops: Vec<Operation>) {
        let mut exchange = Exchange::new();
        for op in ops {
            exchange.apply(&op);
            assert!(exchange.verify_invariants());
        }
    }

    #[test]
    fn deterministic_replay(ops: Vec<Operation>) {
        let mut e1 = Exchange::new();
        for op in &ops { e1.apply(op); }

        let e2 = Exchange::replay(e1.events());
        assert_eq!(e1.best_bid_ask(), e2.best_bid_ask());
        assert_eq!(e1.trades(), e2.trades());
    }
}
```

### 12.4 Benchmark Tests

Using `criterion`:

```rust
// Throughput benchmarks
fn bench_submit_no_match(c: &mut Criterion);
fn bench_submit_with_match(c: &mut Criterion);
fn bench_cancel(c: &mut Criterion);
fn bench_mixed_workload(c: &mut Criterion);
fn bench_deep_book_snapshot(c: &mut Criterion);
```

---

## 13. File Structure

```
rustbook/
├── Cargo.toml
├── README.md
├── SPECS.md
├── LICENSE
├── CHANGELOG.md
├── CONTRIBUTING.md
├── src/
│   ├── lib.rs           # Public API re-exports
│   ├── types.rs         # Price, Quantity, Timestamp, OrderId, TradeId
│   ├── side.rs          # Side enum
│   ├── tif.rs           # TimeInForce enum
│   ├── order.rs         # Order struct and OrderStatus
│   ├── trade.rs         # Trade struct
│   ├── level.rs         # Level (queue at single price)
│   ├── book.rs          # OrderBook (both sides + order index)
│   ├── matching.rs      # Matching engine logic
│   ├── exchange.rs      # Exchange (high-level API)
│   ├── event.rs         # Event enum for replay
│   ├── snapshot.rs      # BookSnapshot, LevelSnapshot
│   └── error.rs         # Error types
├── tests/
│   └── proptest_invariants.rs  # Property-based invariant tests
├── benches/
│   └── throughput.rs    # Performance benchmarks
└── examples/
    ├── basic_usage.rs
    ├── market_making.rs
    └── ioc_execution.rs
```

---

## 14. Dependencies

Minimal dependencies for performance:

```toml
[package]
name = "rustbook"
version = "0.1.0"
edition = "2021"

[dependencies]
thiserror = "2.0"  # Error derive macros

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }
proptest = "1.5"

[features]
default = []
serde = ["dep:serde"]   # Optional serialization
python = ["dep:pyo3"]   # Optional Python bindings

[[bench]]
name = "throughput"
harness = false
```

---

## 15. Future Extensions (v2+)

Potential additions based on demand:

1. **Stop orders**: Trigger on price threshold crossing
2. **Stop-limit orders**: Stop triggers limit order
3. **Multi-symbol**: HashMap<Symbol, Exchange>
4. **ITCH/OUCH parsing**: Replay historical market data
5. **Python bindings**: PyO3 for strategy testing in Python
6. **WebSocket feed**: Real-time book updates
7. **Persistence**: Event sourcing to SQLite/RocksDB
8. **Self-trade prevention**: Block orders from same trader ID
9. **Display quantity**: Iceberg order support
