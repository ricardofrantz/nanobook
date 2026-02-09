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
| `nanobook` | LOB matching engine, portfolio simulator, backtest bridge |
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
nanobook = "0.7"
```

**From source:**

```bash
git clone https://github.com/ricardofrantz/nanobook
cd nanobook
cargo build --release
cargo test

# Python bindings
cd python && maturin develop --release
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
)

print(f"Sharpe: {result['metrics'].sharpe:.2f}")
print(f"Max DD: {result['metrics'].max_drawdown:.1%}")
```

Your optimizer produces weights. `backtest_weights()` handles rebalancing,
cost modeling, position tracking, and return computation at compiled speed
with the GIL released.

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

- **[DOC.md](DOC.md)** — Developer reference: full API, types, patterns, advanced usage
- **[docs.rs](https://docs.rs/nanobook)** — Rust API docs

## License

MIT
