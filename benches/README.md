# nanobook benchmark methodology

Latencies reported in the main README (120 ns submit, 8M ops/sec) are
single-threaded microbenchmarks measured with `criterion` on a warm
cache. They are NOT end-to-end live-trading latencies.

## Methodology

- **Platform.** AMD Ryzen / Intel Core at stock clocks. CPU frequency
  scaling left at default, not pinned.
- **Cache state.** Warm cache. Each criterion sample pre-allocates the
  exchange state, then measures repeated operations on an already-hot
  data structure.
- **Concurrency.** Single-threaded, single-writer.
- **Matching.** The `Submit (no match)` number reflects adding a level
  to an empty or non-crossing book. `Submit (with match)` adds the cost
  of one matching trade.
- **Exclusions.** No network, no serialization, no persistence, no
  risk-engine check, no Python interop.

## What The Numbers Mean

- **Lower bound** for end-to-end live latency. A production deployment
  that adds broker network round-trip, risk checks, and audit logging
  will measure latency in microseconds to milliseconds, not nanoseconds.
- **Useful for** comparing algorithmic changes within nanobook, such as
  checking whether a refactor regressed matching-hot-path cost.
- **Not useful for** direct comparison against Chronicle (IPC ring),
  OnixS (FIX handler), or NautilusTrader, which publishes no public
  submit-latency table.

## Reproducing

```bash
cargo bench --bench throughput
# Results in target/criterion/report/index.html
```

Report regressions via GitHub issue with the `target/criterion/`
output attached.
