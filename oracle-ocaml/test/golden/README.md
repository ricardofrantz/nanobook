# Golden Corpus for OCaml Oracle Testing

This directory contains golden corpus test cases for differential testing between the Rust nanobook implementation and the OCaml oracle.

## Test Cases

### basic_cross.jsonl
Tests basic price crossing:
- Two sell orders at different price levels
- One buy order that crosses the best ask
- Expected: One trade at the crossing price

### fok_no_match.jsonl
Tests FOK (Fill-or-Kill) with no match:
- One sell order resting
- One buy order with FOK that doesn't cross
- Expected: No trades (order rejected)

### partial_fill.jsonl
Tests partial order fill:
- One sell order for 50 units
- One buy order for 100 units at same price
- Expected: One trade for 50 units, buy order rests with 50 remaining

## Adding New Test Cases

To add a new golden corpus test:

1. Create `test_name.jsonl` with input events
2. Create `test_name_expected.jsonl` with expected trade output
3. Document the edge case being tested
4. Ensure both Rust and OCaml implementations produce byte-identical output

## Validation

The CI oracle job should:
1. Replay each golden corpus input through both implementations
2. Compare trade outputs byte-by-byte
3. Fail the build on any divergence

## Future Test Cases

Planned additions:
- Cancel at zero quantity
- Multiple fills at same price level  
- FOK partial-cross scenarios
- Self-trade prevention with each StpPolicy variant
- Orders at min/max price
- Simultaneous-timestamp events
- Market orders with multiple price levels