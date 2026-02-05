# PLAN.md — Implementation Plan

**Project**: limit-order-book-rs
**Date**: 2026-02-05
**Status**: Not started

---

## Philosophy

- Build bottom-up: types → structures → logic → API
- Test each layer before building on it
- Keep it simple — resist premature optimization
- Specs are guidelines, not commandments — adjust as we learn

---

## Phase 1: Project Setup & Core Types

### 1.1 Cargo.toml
```toml
[package]
name = "limit-order-book"
version = "0.1.0"
edition = "2021"

[dependencies]
thiserror = "2.0"

[dev-dependencies]
criterion = { version = "0.5", features = ["html_reports"] }

[[bench]]
name = "throughput"
harness = false
```

### 1.2 src/lib.rs
- Re-export public API
- Keep minimal, just wiring

### 1.3 src/types.rs
```rust
pub struct Price(pub i64);
pub type Quantity = u64;
pub type Timestamp = u64;
pub struct OrderId(pub u64);
pub struct TradeId(pub u64);
```

Implement for each:
- `Clone, Copy, Debug, PartialEq, Eq, Hash`
- `PartialOrd, Ord` for Price
- `Display` for human-readable output

### 1.4 src/side.rs
```rust
pub enum Side { Buy, Sell }
```
- `opposite()` method
- Derive standard traits

### 1.5 src/tif.rs
```rust
pub enum TimeInForce { GTC, IOC, FOK }
```
- Default = GTC

**Tests**: Unit tests for Price ordering, Side::opposite()

**Checkpoint**: `cargo test` passes

---

## Phase 2: Order & Trade

### 2.1 src/order.rs
```rust
pub enum OrderStatus { New, PartiallyFilled, Filled, Cancelled }

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

Methods:
- `is_active() -> bool`
- `fill(qty: Quantity)` — updates remaining/filled/status

### 2.2 src/trade.rs
```rust
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

**Tests**: Order fill logic, status transitions

**Checkpoint**: `cargo test` passes

---

## Phase 3: Level (Queue at Single Price)

### 3.1 src/level.rs
```rust
pub struct Level {
    price: Price,
    orders: VecDeque<OrderId>,
    total_quantity: Quantity,
}
```

Methods:
- `new(price: Price) -> Self`
- `push_back(order_id: OrderId, qty: Quantity)`
- `front() -> Option<OrderId>`
- `pop_front(qty: Quantity)` — decrements total
- `remove(order_id: OrderId, qty: Quantity)` — for cancel mid-queue
- `is_empty() -> bool`
- `total_quantity() -> Quantity`

**Design Decision**: Store `OrderId` not `Order` — orders live in central HashMap for O(1) lookup. Level just tracks queue order.

**Tests**: FIFO behavior, quantity tracking

**Checkpoint**: `cargo test` passes

---

## Phase 4: PriceLevels (One Side of Book)

### 4.1 src/book.rs (partial)
```rust
pub struct PriceLevels {
    levels: BTreeMap<Price, Level>,
    best_price: Option<Price>,
    side: Side,
}
```

Methods:
- `new(side: Side) -> Self`
- `best_price() -> Option<Price>` — O(1) cached
- `best_level() -> Option<&Level>`
- `best_level_mut() -> Option<&mut Level>`
- `get_or_create_level(price: Price) -> &mut Level`
- `remove_level(price: Price)` — updates best_price cache
- `insert_order(price: Price, order_id: OrderId, qty: Quantity)`

**Key Logic**:
- Bids: best = highest price → `levels.last_key_value()`
- Asks: best = lowest price → `levels.first_key_value()`

**Tests**: Best price tracking, level creation/removal

**Checkpoint**: `cargo test` passes

---

## Phase 5: OrderBook

### 5.1 src/book.rs (complete)
```rust
pub struct OrderBook {
    bids: PriceLevels,
    asks: PriceLevels,
    orders: HashMap<OrderId, Order>,
    next_order_id: u64,
    next_trade_id: u64,
    next_timestamp: u64,
}
```

Methods:
- `new() -> Self`
- `next_order_id() -> OrderId`
- `next_trade_id() -> TradeId`
- `next_timestamp() -> Timestamp`
- `get_order(id: OrderId) -> Option<&Order>`
- `get_order_mut(id: OrderId) -> Option<&mut Order>`
- `add_order(order: Order)` — inserts into HashMap + appropriate Level
- `remove_order(id: OrderId) -> Option<Order>` — removes from Level + returns
- `best_bid() -> Option<Price>`
- `best_ask() -> Option<Price>`

**Tests**: Add/remove orders, best price updates

**Checkpoint**: `cargo test` passes

---

## Phase 6: Matching Engine

### 6.1 src/matching.rs
```rust
pub struct MatchResult {
    pub trades: Vec<Trade>,
    pub remaining_quantity: Quantity,
}

impl OrderBook {
    pub fn match_order(&mut self, order: &mut Order) -> MatchResult;
}
```

Core logic:
1. Get opposite side's best level
2. Check price crossing
3. Fill against resting orders (FIFO)
4. Create Trade for each fill
5. Remove exhausted orders/levels
6. Repeat until no cross or order filled

**Tests**:
- No match (prices don't cross)
- Full match (single resting order)
- Partial match (order larger than liquidity)
- Multi-level match (sweeps through prices)
- Price improvement (trade at resting price)

**Checkpoint**: `cargo test` passes, matching works correctly

---

## Phase 7: Exchange API

### 7.1 src/exchange.rs
```rust
pub struct Exchange {
    book: OrderBook,
    trades: Vec<Trade>,
    events: Vec<Event>,
}
```

Methods:
- `new() -> Self`
- `submit_limit(side, price, qty, tif) -> SubmitResult`
- `submit_market(side, qty) -> SubmitResult`
- `cancel(order_id) -> CancelResult`
- `modify(order_id, new_price, new_qty) -> ModifyResult`
- `get_order(id) -> Option<&Order>`
- `best_bid_ask() -> (Option<Price>, Option<Price>)`
- `depth(n) -> BookSnapshot`
- `full_book() -> BookSnapshot`
- `trades() -> &[Trade]`
- `events() -> &[Event]`

### 7.2 src/result.rs
```rust
pub struct SubmitResult { ... }
pub struct CancelResult { ... }
pub struct ModifyResult { ... }
```

### 7.3 src/snapshot.rs
```rust
pub struct BookSnapshot { ... }
pub struct LevelSnapshot { ... }
```

### 7.4 TIF Implementation

**GTC**: Match, rest remainder
**IOC**: Match, cancel remainder (don't add to book)
**FOK**: Check if fully fillable first, then match or reject

**Tests**:
- Full order flow (submit → partial fill → cancel)
- IOC behavior
- FOK all-or-nothing
- Modify loses priority

**Checkpoint**: `cargo test` passes, full API working

---

## Phase 8: Event Log & Replay

### 8.1 src/event.rs
```rust
pub enum Event {
    SubmitLimit { ... },
    SubmitMarket { ... },
    Cancel { ... },
    Modify { ... },
}

impl Exchange {
    pub fn replay(events: &[Event]) -> Self;
    pub fn apply(&mut self, event: &Event) -> ApplyResult;
}
```

**Tests**:
- Replay produces identical state
- Property test: random ops → replay → same result

**Checkpoint**: Determinism guaranteed

---

## Phase 9: Error Handling

### 9.1 src/error.rs
```rust
#[derive(Debug, thiserror::Error)]
pub enum ValidationError { ... }

#[derive(Debug, thiserror::Error)]
pub enum CancelError { ... }

#[derive(Debug, thiserror::Error)]
pub enum ModifyError { ... }
```

Add `try_submit_limit`, `try_submit_market` variants that return `Result`.

**Checkpoint**: Errors are typed and informative

---

## Phase 10: Benchmarks & Optimization

### 10.1 benches/throughput.rs
```rust
fn bench_submit_no_match(c: &mut Criterion);
fn bench_submit_with_match(c: &mut Criterion);
fn bench_cancel(c: &mut Criterion);
fn bench_mixed_workload(c: &mut Criterion);
```

### 10.2 Optimization (if needed)
- Profile with `cargo flamegraph`
- Check: Is cancel O(1)? May need indexed VecDeque
- Check: HashMap vs BTreeMap for order storage
- Check: Inline small types

**Target**: >1M orders/sec

**Checkpoint**: `cargo bench` shows acceptable performance

---

## Phase 11: Documentation & Examples

### 11.1 examples/
- `basic_usage.rs` — Simple buy/sell flow
- `market_making.rs` — Two-sided quoting
- `ioc_execution.rs` — Aggressive IOC pattern

### 11.2 Doc comments
- `//!` module docs
- `///` function docs with examples

**Checkpoint**: `cargo doc --open` looks good

---

## Summary: Build Order

| Phase | Deliverable | Est. Complexity |
|-------|-------------|-----------------|
| 1 | Types + project setup | Low |
| 2 | Order + Trade | Low |
| 3 | Level | Low |
| 4 | PriceLevels | Medium |
| 5 | OrderBook | Medium |
| 6 | Matching engine | High |
| 7 | Exchange API | Medium |
| 8 | Events/replay | Medium |
| 9 | Error handling | Low |
| 10 | Benchmarks | Medium |
| 11 | Docs/examples | Low |

---

## Open Questions (Decide As We Go)

1. **Cancel from mid-queue**: O(n) scan of VecDeque, or maintain OrderId→index map for O(1)?
   - Start simple (O(n)), optimize if benchmarks show need

2. **Order storage**: Keep all orders forever, or prune completed?
   - Start with keeping all (simpler, enables historical queries)
   - Add pruning later if memory becomes issue

3. **FOK simulation**: Clone the book, or just calculate available liquidity?
   - Calculate liquidity (cheaper) — sum quantities at crossing prices

4. **Thread safety**: Single-threaded first, add `Arc<Mutex<>>` wrapper later?
   - Yes, single-threaded. User can wrap if needed.

---

## Notes

- Run `cargo test` after each phase
- Commit working states frequently
- If something feels wrong, stop and reconsider — specs can change
