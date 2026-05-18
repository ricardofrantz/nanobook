# OCaml Oracle v0.15 Implementation Summary

## Overview
Implemented a complete OCaml limit-order-book reference oracle for differential testing against the Rust nanobook implementation. The oracle provides a deterministic reference implementation that can be used to verify correctness of the Rust engine.

## Architecture
- **Language**: OCaml 5.4.1
- **Build System**: dune 3.16
- **Dependencies**: yojson (JSON), crowbar (property-based testing)
- **Design**: Sorted association lists for price levels (deliberately slow but obvious for correctness)

## Core Components

### Type Modules
- **price.ml**: Fixed-point int64 cents representation with display formatting
- **side.ml**: Buy/Sell enum with opposite() helper
- **order.ml**: Order lifecycle with time-in-force (GTC, IOC, FOK) and status tracking
- **book.ml**: Sorted association lists for price levels, central Hashtbl order index
- **matching.ml**: STP policies (Off, CancelNewest, CancelOldest, DecrementAndCancel), FIFO matching at price levels
- **replay.ml**: Event types (SubmitLimit, SubmitMarket, Cancel), replay logic
- **json.ml**: JSONL parsing/writing with Yojson

### CLI Binary
- **replay.exe**: Reads JSONL events, emits JSONL trades
- Fully functional with proper module access
- Supports all event types and STP policies

## Key Features Implemented

### 1. Matching Engine
- FIFO execution at price levels
- Price crossing logic (buy >= ask, sell <= bid)
- Time-in-force enforcement:
  - GTC: Rests on book until filled or cancelled
  - IOC: Immediate execution, remainder cancelled
  - FOK: Fill completely or cancel entirely (no partial fills)

### 2. Self-Trade Prevention (STP)
- Four STP policies fully implemented:
  - **Off**: Orders from same owner can trade freely
  - **CancelNewest**: Cancel incoming order on self-trade
  - **CancelOldest**: Cancel resting order on self-trade
  - **DecrementAndCancel**: Cancel smaller order; if equal, cancel resting
- Owner-based STP detection via optional owner field

### 3. FOK Implementation
- Pre-trade liquidity calculation to determine if FOK can fill completely
- Cancels without trades if insufficient liquidity
- Proceeds normally if sufficient liquidity available

### 4. JSONL Format
- Event schema: SubmitLimit, SubmitMarket, Cancel
- Trade schema with aggressor/passive order IDs and sides
- Optional owner and STP policy fields
- Backward compatible (owner and stp_policy default to None/Off)

## Golden Corpus
16 comprehensive test cases in `oracle-ocaml/test/corpus/`:

### Basic Functionality
1. **01-simple-cross**: Basic limit order crossing
2. **02-no-cross**: Orders with spread, no trades
3. **03-market-order-sweep**: Market order sweeps multiple price levels

### Time-in-Force
4. **04-fok-no-match**: FOK order with no matches
5. **05-fok-partial-cross**: FOK order cannot fully fill
6. **06-ioc-partial-fill**: IOC order partial fill with remainder cancel
7. **10-fok-full-fill**: FOK order can fully fill

### Order Management
8. **07-multiple-same-price**: FIFO execution at same price level
9. **08-cancel-resting**: Cancel resting order
10. **09-cancel-partially-filled**: Cancel after partial fill

### Owner Support
11. **11-owner-basic**: Orders with owner IDs (STP disabled)

### Self-Trade Prevention
12. **12-stp-off**: STP disabled, same-owner orders trade
13. **13-stp-cancel-newest**: Incoming order cancelled on self-trade
14. **14-stp-cancel-oldest**: Resting order cancelled on self-trade
15. **15-stp-decrement**: Smaller incoming order cancelled
16. **16-stp-decrement-equal**: Equal quantities, resting order cancelled

## Bug Fixes

### 1. CLI Binary Module Loading
- **Problem**: Binaries couldn't access library modules at runtime
- **Solution**: Added `(wrapped false)` to lib/dune, renamed library to `oracle_lib`
- **Status**: ✅ Resolved

### 2. Infinite Loop in Matching
- **Problem**: While loop continued forever when no liquidity or prices don't cross
- **Solution**: Added `continue_matching` flag to properly terminate loop
- **Status**: ✅ Resolved

### 3. FOK Partial-Fill Bug
- **Problem**: FOK orders were allowing partial fills instead of cancelling
- **Solution**: Added pre-trade liquidity calculation, cancel if insufficient
- **Status**: ✅ Resolved

### 4. STP CancelNewest Bug
- **Problem**: CancelNewest was filling order before cancelling, causing terminal state error
- **Solution**: Cancel incoming order directly without filling first
- **Status**: ✅ Resolved

## Documentation

### Event-Log Schema
- Complete JSONL schema specification in `docs/event-log-schema.md`
- Documents all event types, fields, and STP policies
- Includes validation rules and examples

### CI/CD
- GitHub Actions workflow in `.github/workflows/oracle.yml`
- Tests OCaml oracle build and library
- Ready for Rust integration and differential testing

## Usage

### CLI
```bash
cd oracle-ocaml
opam exec -- dune exec bin/replay_bin.exe -- input.jsonl output.jsonl
```

### Library (OCaml REPL)
```ocaml
#use "oracle_lib";;
let events = Json.parse_jsonl_file "input.jsonl";;
let trades = Replay.replay_events events;;
Json.write_trades_jsonl "output.jsonl" trades;;
```

## Integration with Rust
The OCaml oracle is designed for differential testing:
1. Both implementations read same JSONL event log
2. Both emit JSONL trade output
3. Compare outputs byte-for-byte to verify correctness
4. Any discrepancy indicates a bug in one implementation

## Future Work
- Add market order price validation (currently uses max/min int64)
- Add order priority classes (not in current spec)
- Add iceberg order support (not in current spec)
- Performance optimization if needed (currently uses slow but correct data structures)

## Status
✅ **v0.15 Complete** - All core functionality implemented, tested, and documented.
