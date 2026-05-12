# ITCH Replay Learnings (v0.11)

## Context

v0.11 introduced a reproducible ITCH replay harness to measure end-to-end parsing and order-book update performance on real NASDAQ TotalView-ITCH data. This document captures what surprised us, what we fixed, and what remains for future work.

## What Surprised Us

### Warmup Events Inflate Performance Numbers

Initial performance measurements (p50=875ns parse, p50=1,292ns book-update) were 6-10× slower than steady-state performance. The cause: warmup events (JIT compilation, cache cold starts, allocator warmup) were included in percentile calculations.

**Fix**: Added `--warmup N` flag to `itch-replay` example (default N=1000). First N events now write `null` latencies to the JSONL output, and `report.py` filters these out using `isinstance(value, int) and not isinstance(value, bool)`.

**Result**: Steady-state performance is p50=83ns parse, p50=208ns book-update (10.5× and 6.2× faster respectively).

### JSON File Corruption on Interrupt

The event-log.jsonl file can end with an incomplete JSON line if the replay process is interrupted or crashes. This causes `report.py` to fail with a JSONDecodeError.

**Workaround**: Manually remove the incomplete last line (`head -n N file.jsonl > file.jsonl.tmp && mv`).

**Future**: The replay harness should use atomic file writes (write to temp, rename on completion) or ensure proper flushing/closing on exit.

### Python Environment Setup Friction

Running `report.py` required manual virtual environment setup and package installation:

```bash
cd examples/itch-replay
uv venv
uv pip install -e ../../python
uv run report.py --input data/replay-v2/event-log.jsonl --output data/replay-v2/report.html
```

**Future**: Add a `Makefile` or shell script in `examples/itch-replay/` to automate environment setup and report generation.

## ITCH Parser Gaps Fixed in v0.11

### Null-Safety for Boolean-as-Int Bug

Python's `isinstance(True, int)` returns `True`, which caused the initial latency filter to incorrectly include boolean values. The replay harness never emits true/false for latency fields, but the filter needed to be defensive.

**Fix**: Updated `report.py` latency extraction to use:

```python
if isinstance(parse_value, int) and not isinstance(parse_value, bool):
    parse_latencies.append(parse_value)
```

This ensures only actual integer latencies are included in percentile calculations.

### Book Snapshot Frequency Control

Initial implementation wrote a book snapshot for every event, which was unnecessary for most analysis and bloated the JSONL output.

**Fix**: Added `--snapshot-every N` flag to gate snapshot generation (default N=1000). Only every Nth event includes the full book state.

## Performance Characteristics

### Steady-State Latency

On Apple M1 Pro, 16GB RAM, N=974,288 events (1-minute NASDAQ window, warmup excluded):

| Stage | p50 | p95 | p99 |
|-------|-----|-----|-----|
| ITCH parse | 83 ns | 125 ns | 250 ns |
| LOB book-update | 208 ns | 833 ns | 3,000 ns |

### Throughput Implications

- **Parse throughput**: ~12M events/sec (1 / 83ns)
- **Book-update throughput**: ~4.8M events/sec (1 / 208ns)

The book-update path is more expensive due to order matching, price level management, and trade generation.

## Reproducibility Approach

### CI Smoke Job

Added GitHub Actions job (`examples-smoke`) that:
1. Downloads ITCH data from GitHub releases (cached)
2. Slices to 1-minute window using `dd`
3. Runs replay with warmup exclusion
4. Generates HTML report
5. Validates output file existence

This ensures performance regressions are caught in CI and provides a reproducible benchmark for contributors.

### Data Caching

ITCH data file (~50MB) is cached via GitHub Actions cache to avoid re-downloading on every CI run. Cache key includes the file SHA256 to invalidate when data changes.

### Deterministic Replay

The replay harness is deterministic: same input file, same command-line flags, same output. This enables:
- Performance regression detection
- Cross-platform comparison
- Historical performance tracking

## Candidate v0.12 Follow-ups

### Streamlined Report Generation

Add a convenience script or Makefile target in `examples/itch-replay/`:

```makefile
report:
	uv venv
	uv pip install -e ../../python
	uv run report.py --input data/replay-v2/event-log.jsonl --output data/replay-v2/report.html
```

### Atomic File Writes

Modify the replay harness to write to a temporary file and rename on completion to prevent partial JSON lines on crash/interrupt.

### Additional Metrics

Consider adding:
- **Memory usage**: Peak RSS during replay
- **CPU utilization**: Single-threaded vs potential parallelization
- **Allocation rate**: Rust heap allocations per event (using `dhat` or similar)

### Extended Time Windows

Current benchmark uses a 1-minute window (09:30-09:31 ET). Consider adding:
- **5-minute window**: Captures more varied market conditions
- **Pre-market / after-hours**: Different message rate profiles
- **Multiple symbols**: Cross-symbol book management overhead

## References

- Implementation: `examples/itch-replay/replay.rs`
- Report generator: `examples/itch-replay/report.py`
- CI job: `.github/workflows/ci.yml` (examples-smoke)
- Data: NASDAQ TotalView-ITCH 2019-07-30
- Reproducibility: `REPRODUCIBILITY.md`