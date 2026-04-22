//! Compile-time verification for S9: `BinanceBroker` and
//! `BinanceClient` implement [`zeroize::ZeroizeOnDrop`], which
//! ensures their `api_key` / `secret_key` fields are scrubbed in
//! memory when the owning struct drops.
//!
//! # Why a compile-time test and not a runtime memory-inspection
//! test?
//!
//! A runtime test would read the raw bytes of the `String` heap
//! allocation before and after `drop()`, assert they've been
//! zeroed, and conclude the derive is live. That approach is
//! tempting but brittle:
//!
//! - Reading from a freshly-freed allocation is undefined
//!   behaviour in strict Rust terms; it only works incidentally
//!   because system allocators leave freed bytes alone for a
//!   short window.
//! - Some allocators (glibc's `malloc_perturb`, hardened macOS
//!   builds, sanitizer-enabled runs) actively poison freed
//!   memory. A test that reads zeros might be succeeding because
//!   the allocator poisoned the page, not because
//!   `ZeroizeOnDrop` fired — a false positive.
//!
//! The trait-bound assertion below is the idiomatic check.
//! `ZeroizeOnDrop` is a marker trait that `#[derive]` emits
//! together with a concrete `Drop` impl that calls `zeroize()` on
//! every non-`#[zeroize(skip)]` field. A regression that removes
//! the derive immediately breaks this compile-time check — with
//! a clear trait-bound error message — rather than silently
//! leaving the runtime test happy on a poison-enabled allocator.

#![cfg(feature = "binance")]

use nanobook_broker::binance::{BinanceBroker, client::BinanceClient};
use zeroize::ZeroizeOnDrop;

/// Compile-time trait-bound check. If either type loses its
/// `ZeroizeOnDrop` derive, this function fails to compile with a
/// clear "trait bound … is not satisfied" error pointing here.
fn assert_zeroize_on_drop<T: ZeroizeOnDrop>() {}

#[test]
fn binance_broker_is_zeroize_on_drop() {
    assert_zeroize_on_drop::<BinanceBroker>();
}

#[test]
fn binance_client_is_zeroize_on_drop() {
    assert_zeroize_on_drop::<BinanceClient>();
}
