# ITCH Replay Example

This example demonstrates nanobook's limit-order-book (LOB) processing performance on real NASDAQ TotalView-ITCH 5.0 market data. It streams a full trading day through the LOB, emits a JSONL event log, and generates a performance report with latency histograms.

## What this shows

- **Real-world performance**: Measured latency on actual exchange data (not synthetic benchmarks)
- **LOB reconstruction**: Full order-book state tracking with top-of-book spread analysis
- **Invariant validation**: Self-checks ensure book integrity (no crossed book, volume conservation, etc.)
- **Reproducible**: Anyone can reproduce these results following the steps below

## Quick start

```bash
# Download NASDAQ ITCH sample (≈3.5 GB compressed)
./download.sh

# Run replay (emits JSONL event log + summary stats)
cargo run --release --example itch-replay

# Generate HTML report
uv run python report.py

# View report
open data/report-v2/report.html
```

Expected runtime on a 16 GB laptop: **<10 minutes** for the full trading day.

## Data source

- **Source**: NASDAQ TotalView-ITCH 5.0 (public sample)
- **Date**: 2019-07-30
- **Download**: `emi.nasdaq.com` (via `download.sh`)
- **License**: Free redistribution varies; data is fetched at runtime, not vendored
- **Checksum**: Verified via SHA256 in `download.sh`

## Output files

After running the replay:

```
data/
├── it-events.jsonl          # Full event log (one line per LOB event)
├── it-summary.json          # Summary statistics (message counts, latency percentiles)
├── it-invariants.log        # LOB invariant check results (empty if all pass)
└── report-v2/
    └── report.html          # Performance report with histograms
```

## Invariant checks

The replay validates LOB integrity throughout:

- **No crossed book**: Bid prices never exceed ask prices at any tick
- **Monotonic timestamps**: Per-symbol timestamps never decrease
- **Cancel correctness**: Every cancel reduces resting quantity (or removes the order)
- **Volume conservation**: Aggregate volume conserved across event boundaries

If any invariant violation occurs, it's logged to `it-invariants.log` with details.

## Performance methodology

- **Warmup**: First 100,000 events excluded from latency pools (avoids allocator warmup bias)
- **Single-threaded**: No parallelism; measures core LOB performance
- **Measured events**: N=973,285 for the reported 1-minute slice (09:30-09:31 ET)
- **Hardware**: Apple M1 Pro, 16 GB RAM (see `REPRODUCIBILITY.md` for full environment)

For full reproducibility details (exact versions, hardware specs, dependency hashes), see `REPRODUCIBILITY.md` at the repo root.

## CI slice

GitHub Actions runs a deterministic 1-minute slice on every PR via the `examples-smoke` job. This validates the replay pipeline without downloading the full 3.5 GB dataset.

The slice is generated from the full dataset and byte-identical across runs. Expected outputs are in `expected/`.

## Report contents

The HTML report (`report.html`) includes:

- **Latency histograms**: p50/p95/p99 for ITCH parse, LOB book-update, and strategy-to-order stages
- **Spread distribution**: Top-of-book bid-ask spread over time
- **Message rate timeline**: Events per second throughout the trading day
- **Summary statistics**: Total messages, trades, executions, cancels, etc.

## Learnings

See `docs/solutions/itch-replay-learnings.md` for surprises, residual issues, and follow-up candidates identified during this work.

## Troubleshooting

**Download fails**: Ensure you have internet access and sufficient disk space (≈10 GB free for the compressed + expanded data).

**Replay crashes**: Check that the ITCH file downloaded correctly (`sha256sum -c expected/sample.sha256`).

**Report generation fails**: Ensure Python dependencies are installed via `uv pip install -r requirements.txt`.

**Invariant violations**: Check `it-invariants.log` for details. This may indicate a bug in the LOB or ITCH parser.