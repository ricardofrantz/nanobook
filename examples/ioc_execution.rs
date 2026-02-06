// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! IOC execution example: sweep liquidity with IOC orders, demonstrate partial fills.
//!
//! Run with: cargo run --example ioc_execution

use nanobook::{Exchange, Price, Side, TimeInForce};

fn main() {
    let mut exchange = Exchange::new();

    println!("=== IOC Execution Demo ===\n");

    // Build a multi-level ask book
    println!("Building ask book:");
    let levels = [(100_00, 50), (100_50, 75), (101_00, 100), (102_00, 200)];
    for (price, qty) in &levels {
        exchange.submit_limit(Side::Sell, Price(*price), *qty, TimeInForce::GTC);
        println!("  ASK {} @ ${:.2}", qty, *price as f64 / 100.0);
    }

    // IOC that partially fills — gets what's available, cancels rest
    println!("\n--- IOC buy 100 @ $100.50 ---");
    let result = exchange.submit_limit(Side::Buy, Price(100_50), 100, TimeInForce::IOC);
    println!("  Status:    {:?}", result.status);
    println!("  Filled:    {}", result.filled_quantity);
    println!("  Cancelled: {}", result.cancelled_quantity);
    println!("  Resting:   {}", result.resting_quantity);
    for trade in &result.trades {
        println!("  Trade: {} @ {}", trade.quantity, trade.price);
    }

    // IOC with no available liquidity at that price
    println!("\n--- IOC buy 50 @ $99.00 (no match) ---");
    let result = exchange.submit_limit(Side::Buy, Price(99_00), 50, TimeInForce::IOC);
    println!("  Status:    {:?}", result.status);
    println!("  Filled:    {}", result.filled_quantity);
    println!("  Cancelled: {}", result.cancelled_quantity);

    // IOC sweep across multiple levels
    println!("\n--- IOC buy 200 @ $102.00 (multi-level sweep) ---");
    let result = exchange.submit_limit(Side::Buy, Price(102_00), 200, TimeInForce::IOC);
    println!("  Status:    {:?}", result.status);
    println!("  Filled:    {}", result.filled_quantity);
    println!("  Cancelled: {}", result.cancelled_quantity);
    println!("  Trades:    {}", result.trades.len());
    for trade in &result.trades {
        println!("  Trade: {} @ {}", trade.quantity, trade.price);
    }

    // Compare with FOK — all or nothing
    println!("\n--- FOK buy 300 @ $102.00 (not enough liquidity) ---");
    let result = exchange.submit_limit(Side::Buy, Price(102_00), 300, TimeInForce::FOK);
    println!("  Status:    {:?}", result.status);
    println!("  Filled:    {}", result.filled_quantity);
    println!("  Cancelled: {}", result.cancelled_quantity);
    println!(
        "  Trades:    {} (FOK rejected — ask book untouched)",
        result.trades.len()
    );

    // Final state
    println!("\n=== Final Book State ===");
    let snap = exchange.depth(10);
    for level in &snap.asks {
        println!(
            "  ASK {} @ ${:.2}",
            level.quantity,
            level.price.0 as f64 / 100.0
        );
    }
    if snap.asks.is_empty() {
        println!("  (no asks remaining)");
    }

    println!("\nTotal trades: {}", exchange.trades().len());
}
