// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Throughput benchmarks for limit order book operations.
//!
//! Measures performance of core operations:
//! - Order submission (with and without matching)
//! - Order cancellation
//! - Market order execution
//! - Book queries (BBO, depth)

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use limit_order_book::{Exchange, Price, Side, TimeInForce};

/// Build an exchange with N price levels on each side.
fn build_book(levels: usize, orders_per_level: usize) -> Exchange {
    let mut exchange = Exchange::new();

    // Add bid levels: 99.00, 98.00, 97.00, ...
    for i in 0..levels {
        let price = Price(99_00 - (i as i64) * 100);
        for _ in 0..orders_per_level {
            exchange.submit_limit(Side::Buy, price, 100, TimeInForce::GTC);
        }
    }

    // Add ask levels: 101.00, 102.00, 103.00, ...
    for i in 0..levels {
        let price = Price(101_00 + (i as i64) * 100);
        for _ in 0..orders_per_level {
            exchange.submit_limit(Side::Sell, price, 100, TimeInForce::GTC);
        }
    }

    exchange
}

/// Benchmark: Submit limit order (no match, rests on book)
fn bench_submit_no_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("submit_no_match");

    for levels in [10, 100, 1000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(levels),
            &levels,
            |b, &levels| {
                let mut exchange = build_book(levels, 1);
                let mut price_offset = 0i64;

                b.iter(|| {
                    // Submit at a price that won't match (bid below best bid)
                    let price = Price(50_00 - price_offset);
                    price_offset = (price_offset + 1) % 1000;
                    black_box(exchange.submit_limit(Side::Buy, price, 100, TimeInForce::GTC))
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: Submit limit order that fully matches
fn bench_submit_with_match(c: &mut Criterion) {
    let mut group = c.benchmark_group("submit_with_match");
    group.throughput(Throughput::Elements(1));

    group.bench_function("single_fill", |b| {
        b.iter_batched(
            || {
                let mut exchange = Exchange::new();
                exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
                exchange
            },
            |mut exchange| {
                black_box(exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC))
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark: Cancel order
fn bench_cancel(c: &mut Criterion) {
    let mut group = c.benchmark_group("cancel");

    for levels in [10, 100, 1000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(levels),
            &levels,
            |b, &levels| {
                b.iter_batched(
                    || {
                        let exchange = build_book(levels, 10);
                        // Get an order ID from the middle of the book
                        let order_id = exchange
                            .book()
                            .bids()
                            .best_level()
                            .and_then(|l| l.front())
                            .unwrap();
                        (exchange, order_id)
                    },
                    |(mut exchange, order_id)| black_box(exchange.cancel(order_id)),
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: Market order sweeping multiple levels
fn bench_market_sweep(c: &mut Criterion) {
    let mut group = c.benchmark_group("market_sweep");

    for levels_to_sweep in [1, 5, 10] {
        group.throughput(Throughput::Elements(levels_to_sweep as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(levels_to_sweep),
            &levels_to_sweep,
            |b, &levels| {
                b.iter_batched(
                    || build_book(20, 1),
                    |mut exchange| {
                        // Sweep `levels` price levels (each has 100 qty)
                        let qty = levels as u64 * 100;
                        black_box(exchange.submit_market(Side::Buy, qty))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmark: Best bid/ask query (O(1) operation)
fn bench_bbo_query(c: &mut Criterion) {
    let mut group = c.benchmark_group("bbo_query");

    for levels in [10, 100, 1000] {
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::from_parameter(levels),
            &levels,
            |b, &levels| {
                let exchange = build_book(levels, 10);

                b.iter(|| black_box(exchange.best_bid_ask()));
            },
        );
    }

    group.finish();
}

/// Benchmark: Depth snapshot generation
fn bench_depth_snapshot(c: &mut Criterion) {
    let mut group = c.benchmark_group("depth_snapshot");

    let exchange = build_book(100, 10);

    for depth in [5, 10, 20] {
        group.throughput(Throughput::Elements(depth as u64 * 2)); // Both sides
        group.bench_with_input(BenchmarkId::from_parameter(depth), &depth, |b, &depth| {
            b.iter(|| black_box(exchange.depth(depth)));
        });
    }

    group.finish();
}

/// Benchmark: Replay events (only with event-log feature)
#[cfg(feature = "event-log")]
fn bench_replay(c: &mut Criterion) {
    let mut group = c.benchmark_group("replay");

    for num_events in [100, 1000, 10000] {
        group.throughput(Throughput::Elements(num_events as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(num_events),
            &num_events,
            |b, &num_events| {
                // Build exchange and collect events
                let mut exchange = Exchange::new();
                for i in 0..num_events {
                    let price = Price(100_00 + (i as i64 % 100) * 10);
                    let side = if i % 2 == 0 { Side::Buy } else { Side::Sell };
                    exchange.submit_limit(side, price, 100, TimeInForce::GTC);
                }
                let events = exchange.events().to_vec();

                b.iter(|| black_box(Exchange::replay(&events)));
            },
        );
    }

    group.finish();
}

#[cfg(feature = "event-log")]
criterion_group!(
    benches,
    bench_submit_no_match,
    bench_submit_with_match,
    bench_cancel,
    bench_market_sweep,
    bench_bbo_query,
    bench_depth_snapshot,
    bench_replay,
);

#[cfg(not(feature = "event-log"))]
criterion_group!(
    benches,
    bench_submit_no_match,
    bench_submit_with_match,
    bench_cancel,
    bench_market_sweep,
    bench_bbo_query,
    bench_depth_snapshot,
);

criterion_main!(benches);
