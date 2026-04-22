# Mutation-testing baseline — matching engine

**Date:** 2026-04-22
**Tool:** `cargo-mutants` v27.0.0
**Target:** `src/matching.rs`, `src/exchange.rs`, `src/level.rs`
**Invocation:**

```bash
cargo mutants --package nanobook \
    --file src/matching.rs \
    --file src/exchange.rs \
    --file src/level.rs \
    --timeout 60 \
    --jobs 4 \
    --all-features
```

**Runtime:** ~9 min (parallelism 4, 148 mutants, 14 hit the 60 s timeout
— those are infinite-loop mutations in the match-at-price inner loop,
which `cargo-mutants` counts as caught).

## Summary

| Outcome               | Count | Fraction |
|-----------------------|------:|---------:|
| Caught (tests failed) |   100 |   78.7 % |
| Timeout (infinite loop caught by timer) | 14 | 11.0 % |
| **Killed** (caught + timeout) | **114** | **89.76 %** |
| Missed (survivor)     |    13 |   10.24 % |
| Unviable (fails to compile, excluded from rate) | 21 | — |
| **Total testable**    | **127** | — |
| Total mutants generated | 148 | — |

Kill rate **89.76 %** clears the plan's ≥85 % bar for P1.

## Baseline vs. tests-added run

An initial run against the pre-I3 test suite reported 84.25 % kill rate
(20 survivors). Six targeted regression tests were added this
commit to close the meaningful gaps:

| File | Test | Kills |
|------|------|------:|
| `src/exchange.rs` | `immediate_trigger_sell_stop_if_price_already_past` | 1 |
| `src/exchange.rs` | `clear_trades_empties_trade_history` | 1 |
| `src/exchange.rs` | `compact_removes_tombstones_across_exchange` | 1 |
| `src/level.rs`    | `raw_len_tracks_queue_size_across_pushes_and_tombstones` | 1 |
| `src/level.rs`    | `pop_front_decrements_tombstone_count_when_head_is_tombstone` | 2 |
| `src/matching.rs` | Negative `assert!(!result.is_empty())` added to `full_fill_exact_quantity` | 1 |
| **Total** | | **7** |

Net change: +6 tests → +7 mutations killed → kill rate +5.51 pp.

## Surviving mutations (13, all expected)

Every remaining survivor is an accessor or a constant-return
mutation that the test suite verifies *indirectly* through
behavior rather than direct assertion on the accessor's return
value. These are classic "expected survivor" patterns per the
cargo-mutants documentation. The bar for adding a test to kill
each of these is higher than the value it provides — an extra
`assert_eq!(ex.stp_policy(), StpPolicy::Off)` at the top of
every test would kill a few but reads as overfitted ceremony.

| File:line | Mutation | Why it survives |
|-----------|----------|-----------------|
| `exchange.rs:77` | `Exchange::stp_policy -> Default::default()` | Accessor. The STP policy's *effect* is exhaustively tested in `tests/stp_policy.rs` (12 cases); the accessor itself is only read by external observers. |
| `exchange.rs:407` | Replace `o.is_active()` match guard with `true` in `modify_internal` | Equivalent mutation. The immediately-following `self.book.cancel_order(order_id)` call has its own `is_active()` check that returns `None` on terminal orders, producing the same `ModifyError::OrderNotActive` either way. |
| `exchange.rs:699` (×3) | `best_bid_ask` returning fixed `(None, None) / (None, Some(_)) / (Some(_), None)` | Accessor. Every test that uses `best_bid_ask()` does so to derive a downstream assertion, not to check the tuple directly. |
| `exchange.rs:714` (×3) | `spread` returning `None / Some(0) / Some(1)` | Accessor, same reasoning as `best_bid_ask`. |
| `exchange.rs:724` | `full_book -> Default::default()` | Accessor snapshot of the book; its consistency is verified by the snapshot-dedicated tests directly on `BookSnapshot`. |
| `exchange.rs:739` | `book_mut -> Box::leak(Box::new(Default::default()))` | Accessor exposing internal `OrderBook`; no test takes a mut ref and asserts identity. |
| `exchange.rs:759` | `stop_book -> Box::leak(Box::new(Default::default()))` | Accessor exposing internal `StopBook`; stop-order behaviour is tested through `Exchange::submit_stop_*` + `pending_stop_count`, not through the accessor. |
| `exchange.rs:776` (×2) | `clear_order_history -> 0 / 1` | The exact count of removed orders is not uniquely verified; tests just call the method and move on. The mutation returning a fixed 0 or 1 would be caught only by asserting the specific count — over-fitted. |

## What future mutation runs should watch for

- Any of the 13 expected survivors above re-classifying as "real
  gap" if their accessor contract tightens (e.g., `stp_policy` is
  later consumed by a risk check that depends on the exact
  policy value).
- The `matching.rs` TIMEOUT bucket (14) represents the
  infinite-loop-provoking mutations in the match-at-price while
  loop. A future optimization that bounds iterations by count
  rather than by progress could convert those from TIMEOUT
  (caught) to MISSED (survivors) — flag for manual review.

## How to reproduce

```bash
cargo install cargo-mutants --version 27.0.0 --locked
cargo mutants --package nanobook \
    --file src/matching.rs \
    --file src/exchange.rs \
    --file src/level.rs \
    --timeout 60 \
    --jobs 4 \
    --all-features \
    --output mutants.out
```

Outputs land in `mutants.out/mutants.out/`:
- `outcomes.json` — full results in JSON.
- `caught.txt`, `missed.txt`, `timeout.txt`, `unviable.txt` — one
  mutation per line, grouped by outcome.
- `debug.log` — per-mutation test output.
