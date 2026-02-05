// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Basic usage example: create an exchange, submit orders, inspect state.
//!
//! Run with: cargo run --example basic_usage

use nanobook::{Exchange, Price, Side, TimeInForce};

fn main() {
    let mut exchange = Exchange::new();

    // Build some resting liquidity
    println!("=== Building the order book ===\n");

    exchange.submit_limit(Side::Sell, Price(102_00), 200, TimeInForce::GTC);
    exchange.submit_limit(Side::Sell, Price(101_00), 150, TimeInForce::GTC);
    exchange.submit_limit(Side::Buy, Price(99_00), 100, TimeInForce::GTC);
    exchange.submit_limit(Side::Buy, Price(100_00), 250, TimeInForce::GTC);

    let snap = exchange.depth(10);
    println!("Best bid: {:?}", snap.best_bid());
    println!("Best ask: {:?}", snap.best_ask());
    println!("Spread:   {} cents\n", snap.spread().unwrap());

    // Submit a crossing order
    println!("=== Crossing the spread ===\n");

    let result = exchange.submit_limit(Side::Buy, Price(101_00), 100, TimeInForce::GTC);
    println!(
        "Order #{}: filled {}, resting {}",
        result.order_id.0, result.filled_quantity, result.resting_quantity
    );

    for trade in &result.trades {
        println!("  Trade: {} @ {}", trade.quantity, trade.price);
    }

    // Check book after trade
    println!("\nBest bid: {:?}", exchange.best_bid());
    println!("Best ask: {:?}", exchange.best_ask());

    // Submit a market order
    println!("\n=== Market order sweep ===\n");

    let result = exchange.submit_market(Side::Buy, 200);
    println!(
        "Market buy 200: filled {}, cancelled {}",
        result.filled_quantity, result.cancelled_quantity
    );

    for trade in &result.trades {
        println!("  Trade: {} @ {}", trade.quantity, trade.price);
    }

    // Summary
    println!("\n=== Summary ===\n");
    println!("Total trades: {}", exchange.trades().len());
    println!("Best bid: {:?}", exchange.best_bid());
    println!("Best ask: {:?}", exchange.best_ask());
}
