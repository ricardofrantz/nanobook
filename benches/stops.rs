// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Stop order benchmarks: triggers, cascades, and trailing updates.

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use nanobook::{Exchange, Price, Side, TimeInForce, TrailMethod};

fn bench_stop_trigger(c: &mut Criterion) {
    let mut group = c.benchmark_group("stop_trigger");

    for cascade_depth in [1, 10, 50] {
        group.throughput(Throughput::Elements(cascade_depth as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(cascade_depth),
            &cascade_depth,
            |b, &depth| {
                b.iter_batched(
                    || {
                        let mut exchange = Exchange::new();
                        // Build book levels for the triggered stops to execute against
                        for i in 0..depth {
                            exchange.submit_limit(
                                Side::Sell,
                                Price(100_00 + (i as i64 + 1) * 10),
                                100,
                                TimeInForce::GTC,
                            );
                        }
                        // Add cascading stop orders
                        for i in 0..depth {
                            let trigger_price = Price(100_00 + (i as i64) * 10);
                            exchange.submit_stop_market(Side::Buy, trigger_price, 100);
                        }
                        // A resting ask at 100.00 to trigger the first stop
                        exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC);
                        exchange
                    },
                    |mut exchange| {
                        // This buy order produces a trade at 100.00, triggering the first stop,
                        // which trades at 100.10, triggering the second stop, and so on.
                        black_box(exchange.submit_limit(
                            Side::Buy,
                            Price(100_00),
                            100,
                            TimeInForce::GTC,
                        ))
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }
    group.finish();
}

fn bench_trailing_update(c: &mut Criterion) {
    let mut group = c.benchmark_group("trailing_update");

    group.bench_function("fixed", |b| {
        b.iter_batched(
            || {
                let mut exchange = Exchange::new();
                exchange.submit_trailing_stop_market(
                    Side::Sell,
                    Price(95_00),
                    100,
                    TrailMethod::Fixed(100),
                );
                exchange
            },
            |mut exchange| {
                // Update watermark with a trade
                black_box(exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC));
                black_box(exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("percentage", |b| {
        b.iter_batched(
            || {
                let mut exchange = Exchange::new();
                exchange.submit_trailing_stop_market(
                    Side::Sell,
                    Price(95_00),
                    100,
                    TrailMethod::Percentage(0.01),
                );
                exchange
            },
            |mut exchange| {
                black_box(exchange.submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC));
                black_box(exchange.submit_limit(Side::Buy, Price(100_00), 100, TimeInForce::GTC));
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_stop_trigger, bench_trailing_update);
criterion_main!(benches);
