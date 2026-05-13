# Golden Corpus for OCaml Oracle

This directory contains golden corpus test cases for differential testing between the Rust nanobook implementation and the OCaml oracle.

## Structure

Each test case consists of:
- `input.jsonl`: Event log to replay
- `output.jsonl`: Expected trade output (byte-identical from both implementations)

## Test Cases

### 01-simple-cross
Basic limit order crossing - sell at 10100, buy at 10100 should produce one trade.

### 02-no-cross
Limit orders with spread - sell at 10100, buy at 10000 should produce no trades.

### 03-market-order-sweep
Market order sweeps multiple price levels.

### 04-fok-no-match
FOK order that cannot match - should be cancelled immediately.

### 05-fok-partial-cross
FOK order that partially crosses - should be cancelled if not fully filled.

### 06-ioc-partial-fill
IOC order that partially fills - remainder cancelled.

### 07-multiple-same-price
Multiple orders at same price level with FIFO execution.

### 08-cancel-resting
Cancel a resting order on the book.

### 09-cancel-partially-filled
Cancel a partially filled order.

### 10-fok-full-fill
FOK order that can fully fill - should execute normally.

### 11-owner-basic
Orders with owner IDs (STP disabled) - should trade normally.

### 12-stp-off
STP disabled - orders from same owner can trade freely.

### 13-stp-cancel-newest
STP CancelNewest - incoming order cancelled when self-trade detected.

### 14-stp-cancel-oldest
STP CancelOldest - resting order cancelled when self-trade detected.

### 15-stp-decrement
STP DecrementAndCancel - smaller (incoming) order cancelled.

### 16-stp-decrement-equal
STP DecrementAndCancel - equal quantities, resting order cancelled.

## Running Tests

```bash
# Run OCaml oracle on input
cd oracle-ocaml
opam exec -- dune exec bin/replay_bin.exe -- test/corpus/01-simple-cross/input.jsonl /tmp/ocaml-output.jsonl

# Run Rust oracle on input (when implemented)
cargo run --release --features event-log -- --input test/corpus/01-simple-cross/input.jsonl --output /tmp/rust-output.jsonl

# Compare outputs
diff /tmp/ocaml-output.jsonl test/corpus/01-simple-cross/output.jsonl
diff /tmp/rust-output.jsonl test/corpus/01-simple-cross/output.jsonl
```

## Adding New Test Cases

1. Create `test/corpus/NN-descriptor/` directory
2. Add `input.jsonl` with events
3. Run through OCaml oracle to generate `output.jsonl`
4. Add description to this README
5. Commit both files
