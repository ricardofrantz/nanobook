// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Portfolio backtest example: monthly rebalancing with SimpleFill.
//!
//! Run with: cargo run --features portfolio --example portfolio_backtest

#[cfg(feature = "portfolio")]
fn main() {
    use nanobook::Symbol;
    use nanobook::portfolio::{CostModel, Portfolio, compute_metrics};

    let aapl = Symbol::new("AAPL");
    let msft = Symbol::new("MSFT");
    let goog = Symbol::new("GOOG");

    // Start with $1,000,000 and 5 bps round-trip cost
    let cost_model = CostModel {
        commission_bps: 3,
        slippage_bps: 2,
        min_trade_fee: 1_00, // $1 minimum per trade
    };
    let mut portfolio = Portfolio::new(1_000_000_00, cost_model);

    println!("=== Monthly Rebalancing Backtest ===\n");
    println!("Initial capital: $1,000,000");
    println!("Strategy: 50% AAPL, 30% MSFT, 20% GOOG\n");

    // Target weights
    let targets = [(aapl, 0.5), (msft, 0.3), (goog, 0.2)];

    // Simulated monthly prices (12 months)
    let monthly_prices: Vec<Vec<(Symbol, i64)>> = vec![
        vec![(aapl, 150_00), (msft, 300_00), (goog, 140_00)],
        vec![(aapl, 155_00), (msft, 295_00), (goog, 145_00)],
        vec![(aapl, 148_00), (msft, 310_00), (goog, 138_00)],
        vec![(aapl, 160_00), (msft, 305_00), (goog, 150_00)],
        vec![(aapl, 158_00), (msft, 315_00), (goog, 148_00)],
        vec![(aapl, 165_00), (msft, 320_00), (goog, 155_00)],
        vec![(aapl, 162_00), (msft, 308_00), (goog, 152_00)],
        vec![(aapl, 170_00), (msft, 325_00), (goog, 160_00)],
        vec![(aapl, 168_00), (msft, 330_00), (goog, 158_00)],
        vec![(aapl, 175_00), (msft, 335_00), (goog, 165_00)],
        vec![(aapl, 172_00), (msft, 328_00), (goog, 162_00)],
        vec![(aapl, 180_00), (msft, 340_00), (goog, 170_00)],
    ];

    // Run backtest
    for (month, prices) in monthly_prices.iter().enumerate() {
        // Rebalance to target weights
        portfolio.rebalance_simple(&targets, prices);

        // Record return for this period
        portfolio.record_return(prices);

        let snap = portfolio.snapshot(prices);
        println!(
            "  Month {:>2}: equity = ${:>12.2}  positions = {}",
            month + 1,
            snap.equity as f64 / 100.0,
            snap.num_positions,
        );
    }

    // Compute and display metrics
    println!("\n=== Results ===\n");

    let final_prices = monthly_prices.last().unwrap();
    let snap = portfolio.snapshot(final_prices);

    println!("Final equity:    ${:.2}", snap.equity as f64 / 100.0);
    println!("Cash:            ${:.2}", snap.cash as f64 / 100.0);
    println!(
        "Realized PnL:    ${:.2}",
        snap.total_realized_pnl as f64 / 100.0
    );

    println!("\nWeights:");
    for (sym, w) in &snap.weights {
        println!("  {sym}: {:.1}%", w * 100.0);
    }

    if let Some(metrics) = compute_metrics(portfolio.returns(), 12.0, 0.04 / 12.0) {
        println!("\n{metrics}");
    }
}

#[cfg(not(feature = "portfolio"))]
fn main() {
    eprintln!("This example requires the 'portfolio' feature.");
    eprintln!("Run with: cargo run --features portfolio --example portfolio_backtest");
}
