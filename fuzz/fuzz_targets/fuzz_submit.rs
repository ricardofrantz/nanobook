#![no_main]
//! Fuzz target for the Exchange submit/cancel/modify path (I2).
//!
//! Drives a fresh `Exchange` with an arbitrary sequence of
//! `FuzzAction`s and asserts three invariants after every step:
//!
//! 1. **No panic.** Any panic from the matching engine, level
//!    accounting, or stop-order cascade fails the fuzz run with
//!    a reproducible artifact.
//! 2. **Book never crossed.** `best_bid < best_ask` whenever both
//!    sides are populated.
//! 3. **Order IDs strictly monotonic with submission order.**
//!    Every new submit — whether it rests, fills, or is rejected
//!    (FOK ghost-id contract from N7) — returns an id greater
//!    than the previous submission's.
//!
//! The fuzzer is intentionally NOT run in CI. Run locally or on a
//! dedicated machine — see `fuzz/README.md`.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;
use nanobook::{Exchange, OrderId, Price, Side, TimeInForce};

/// Input-bounded side. `arbitrary::Arbitrary` can't derive
/// directly on the `Side` enum from nanobook (foreign type), so
/// we mirror it locally.
#[derive(Debug, Arbitrary)]
enum FuzzSide {
    Buy,
    Sell,
}

impl From<FuzzSide> for Side {
    fn from(s: FuzzSide) -> Self {
        match s {
            FuzzSide::Buy => Side::Buy,
            FuzzSide::Sell => Side::Sell,
        }
    }
}

/// One of the Exchange's public command-entrypoints, driven by
/// arbitrary bytes. Input constraints (price/qty > 0) are applied
/// at dispatch time; invalid values are skipped rather than
/// attempted, because validation rejection is uninteresting to
/// fuzz (the code path through `try_submit_*` is already
/// exercised by unit tests).
#[derive(Debug, Arbitrary)]
enum FuzzAction {
    SubmitLimit {
        side: FuzzSide,
        price: i64,
        qty: u64,
        tif_selector: u8,
    },
    SubmitMarket {
        side: FuzzSide,
        qty: u64,
    },
    Cancel {
        order_id: u64,
    },
    Modify {
        order_id: u64,
        new_price: i64,
        new_qty: u64,
    },
}

fn tif_from_byte(b: u8) -> TimeInForce {
    match b % 3 {
        0 => TimeInForce::GTC,
        1 => TimeInForce::IOC,
        _ => TimeInForce::FOK,
    }
}

fn assert_book_not_crossed(ex: &Exchange) {
    if let (Some(bid), Some(ask)) = ex.best_bid_ask() {
        assert!(bid < ask, "crossed book: bid={} ask={}", bid.0, ask.0);
    }
}

fuzz_target!(|actions: Vec<FuzzAction>| {
    let mut ex = Exchange::new();
    let mut last_submitted_id: u64 = 0;

    // Cap the action count: libFuzzer feeds long inputs naturally,
    // but 256 steps is well past what small-state bugs need to
    // surface, and bounds the per-iteration cost.
    for action in actions.into_iter().take(256) {
        match action {
            FuzzAction::SubmitLimit {
                side,
                price,
                qty,
                tif_selector,
            } => {
                if price <= 0 || qty == 0 {
                    continue;
                }
                let r =
                    ex.submit_limit(side.into(), Price(price), qty, tif_from_byte(tif_selector));
                assert!(
                    r.order_id.0 > last_submitted_id,
                    "order IDs not monotonic: last={last_submitted_id} new={}",
                    r.order_id.0,
                );
                last_submitted_id = r.order_id.0;
            }
            FuzzAction::SubmitMarket { side, qty } => {
                if qty == 0 {
                    continue;
                }
                let r = ex.submit_market(side.into(), qty);
                assert!(
                    r.order_id.0 > last_submitted_id,
                    "order IDs not monotonic: last={last_submitted_id} new={}",
                    r.order_id.0,
                );
                last_submitted_id = r.order_id.0;
            }
            FuzzAction::Cancel { order_id } => {
                let _ = ex.cancel(OrderId(order_id));
            }
            FuzzAction::Modify {
                order_id,
                new_price,
                new_qty,
            } => {
                if new_price <= 0 || new_qty == 0 {
                    continue;
                }
                let _ = ex.modify(OrderId(order_id), Price(new_price), new_qty);
            }
        }

        assert_book_not_crossed(&ex);
    }
});
