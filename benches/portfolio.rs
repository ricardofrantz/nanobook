// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Portfolio benchmarks: backtest, sweep, and metrics computation.

use criterion::{BenchmarkId, Criterion, black_box, criterion_group, criterion_main};
use nanobook::portfolio::{CostModel, EqualWeight, compute_metrics, run_backtest};
use nanobook::{Price, Symbol};

fn sym(s: &str) -> Symbol {
    Symbol::new(s)
}

/// Generate a synthetic price series with `n_bars` bars and `n_stocks` stocks.
///
/// Prices start at $100 and drift randomly using a simple deterministic RNG.
fn generate_price_series(n_bars: usize, n_stocks: usize) -> Vec<Vec<(Symbol, i64)>> {
    let symbols: Vec<Symbol> = (0..n_stocks).map(|i| sym(&format!("S{i:03}"))).collect();

    let mut prices = vec![100_00i64; n_stocks];
    let mut series = Vec::with_capacity(n_bars);

    // Simple deterministic PRNG (xorshift32)
    let mut rng_state: u32 = 42;

    for _ in 0..n_bars {
        let bar: Vec<(Symbol, i64)> = symbols
            .iter()
            .zip(prices.iter_mut())
            .map(|(sym, price)| {
                // xorshift32
                rng_state ^= rng_state << 13;
                rng_state ^= rng_state >> 17;
                rng_state ^= rng_state << 5;

                // Random return between -2% and +2%
                let ret = (rng_state % 401) as i64 - 200; // -200..200 bps
                *price = (*price + *price * ret / 10_000).max(1_00); // Floor at $1
                (*sym, *price)
            })
            .collect();
        series.push(bar);
    }

    series
}

/// Benchmark: Single EqualWeight backtest (20 stocks, monthly, 20 years = 240 bars)
fn bench_single_backtest(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/single_backtest");

    let prices_20y = generate_price_series(240, 20);

    group.bench_function("20y_20stocks_monthly", |b| {
        b.iter(|| {
            black_box(run_backtest(
                &EqualWeight,
                &prices_20y,
                10_000_000_00, // $10M
                CostModel::zero(),
                12.0,
                0.0,
            ))
        });
    });

    group.finish();
}

/// Benchmark: compute_metrics on various series lengths
fn bench_compute_metrics(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/compute_metrics");

    for n in [100, 1_000, 5_000] {
        // Generate deterministic returns
        let mut rng_state: u32 = 123;
        let returns: Vec<f64> = (0..n)
            .map(|_| {
                rng_state ^= rng_state << 13;
                rng_state ^= rng_state >> 17;
                rng_state ^= rng_state << 5;
                (rng_state % 201) as f64 / 10_000.0 - 0.01 // -1% to +1%
            })
            .collect();

        group.bench_with_input(BenchmarkId::from_parameter(n), &returns, |b, returns| {
            b.iter(|| black_box(compute_metrics(returns, 252.0, 0.0)));
        });
    }

    group.finish();
}

/// Benchmark: Parameter sweep with varying number of configurations
#[cfg(feature = "parallel")]
fn bench_sweep(c: &mut Criterion) {
    use nanobook::portfolio::sweep::sweep_strategy;

    let mut group = c.benchmark_group("portfolio/sweep");

    // Use a shorter series for sweep benchmarks (5 years monthly = 60 bars)
    let prices = generate_price_series(60, 10);

    for n_params in [100, 1000] {
        let params: Vec<f64> = (0..n_params).map(|i| i as f64 / 100.0).collect();

        group.bench_with_input(
            BenchmarkId::from_parameter(n_params),
            &params,
            |b, params| {
                b.iter(|| {
                    black_box(sweep_strategy(
                        params,
                        &prices,
                        1_000_000_00,
                        CostModel::zero(),
                        12.0,
                        0.0,
                        |_| EqualWeight,
                    ))
                });
            },
        );
    }

    group.finish();
}

/// Benchmark: rebalance_lob vs rebalance_simple
fn bench_lob_rebalance(c: &mut Criterion) {
    let mut group = c.benchmark_group("portfolio/rebalance");

    let symbols = ["AAPL", "GOOG", "MSFT", "AMZN", "META"];
    let targets: Vec<(Symbol, f64)> = symbols.iter().map(|s| (sym(s), 0.2)).collect();
    let prices: Vec<(Symbol, i64)> = symbols.iter().map(|s| (sym(s), 150_00)).collect();

    group.bench_function("simple", |b| {
        b.iter_batched(
            || nanobook::portfolio::Portfolio::new(1_000_000_00, CostModel::zero()),
            |mut p| {
                p.rebalance_simple(&targets, &prices);
                black_box(())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.bench_function("lob", |b| {
        b.iter_batched(
            || {
                let p = nanobook::portfolio::Portfolio::new(1_000_000_00, CostModel::zero());
                let mut multi = nanobook::MultiExchange::new();
                for (s, price) in &prices {
                    // Build deep books for realistic rebalance
                    for i in 0..10 {
                        multi.get_or_create(s).submit_limit(
                            nanobook::Side::Sell,
                            Price(*price + i * 10),
                            1000,
                            nanobook::TimeInForce::GTC,
                        );
                    }
                }
                (p, multi)
            },
            |(mut p, mut multi)| {
                p.rebalance_lob(&targets, &mut multi);
                black_box(())
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

#[cfg(feature = "parallel")]
criterion_group!(
    benches,
    bench_single_backtest,
    bench_compute_metrics,
    bench_sweep,
    bench_lob_rebalance,
);

#[cfg(not(feature = "parallel"))]
criterion_group!(
    benches,
    bench_single_backtest,
    bench_compute_metrics,
    bench_lob_rebalance,
);

criterion_main!(benches);
