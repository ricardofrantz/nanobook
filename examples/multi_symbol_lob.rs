// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Multi-symbol LOB example: three independent order books.
//!
//! Run with: cargo run --example multi_symbol_lob

use nanobook::{MultiExchange, Price, Side, Symbol, TimeInForce, Trade};

fn main() {
    let mut multi = MultiExchange::new();

    let aapl = Symbol::new("AAPL");
    let msft = Symbol::new("MSFT");
    let goog = Symbol::new("GOOG");

    // === Build order books ===
    println!("=== Building multi-symbol order books ===\n");

    // AAPL: tight market
    let ex = multi.get_or_create(&aapl);
    ex.submit_limit(Side::Buy, Price(150_00), 200, TimeInForce::GTC);
    ex.submit_limit(Side::Buy, Price(149_50), 500, TimeInForce::GTC);
    ex.submit_limit(Side::Sell, Price(150_50), 300, TimeInForce::GTC);
    ex.submit_limit(Side::Sell, Price(151_00), 100, TimeInForce::GTC);

    // MSFT: wider spread
    let ex = multi.get_or_create(&msft);
    ex.submit_limit(Side::Buy, Price(300_00), 100, TimeInForce::GTC);
    ex.submit_limit(Side::Sell, Price(302_00), 150, TimeInForce::GTC);
    ex.submit_limit(Side::Sell, Price(303_00), 200, TimeInForce::GTC);

    // GOOG: lots of depth
    let ex = multi.get_or_create(&goog);
    for i in 0..5 {
        ex.submit_limit(Side::Buy, Price(140_00 - i * 50), 100, TimeInForce::GTC);
        ex.submit_limit(Side::Sell, Price(141_00 + i * 50), 100, TimeInForce::GTC);
    }

    // Display all books
    for (sym, bid, ask) in multi.best_prices() {
        println!(
            "  {sym}: bid={} ask={}",
            bid.map(|p| format!("{p}")).unwrap_or_else(|| "---".into()),
            ask.map(|p| format!("{p}")).unwrap_or_else(|| "---".into()),
        );
    }

    // === Execute some trades ===
    println!("\n=== Trading ===\n");

    // Market buy 250 AAPL — sweeps through levels
    let result = multi.get_or_create(&aapl).submit_market(Side::Buy, 250);
    println!("AAPL market buy 250:");
    println!(
        "  Filled: {}, Cancelled: {}",
        result.filled_quantity, result.cancelled_quantity
    );
    for trade in &result.trades {
        println!("  {} @ {}", trade.quantity, trade.price);
    }
    if !result.trades.is_empty() {
        let vwap = Trade::vwap(&result.trades).unwrap();
        println!("  VWAP: {vwap}");
    }

    // Limit buy MSFT — crosses spread
    let result =
        multi
            .get_or_create(&msft)
            .submit_limit(Side::Buy, Price(302_00), 100, TimeInForce::GTC);
    println!("\nMSFT limit buy 100 @ $302:");
    println!(
        "  Filled: {}, Resting: {}",
        result.filled_quantity, result.resting_quantity
    );

    // === Book analytics ===
    println!("\n=== Book Analytics ===\n");

    let snap = multi.get(&goog).unwrap().depth(5);
    println!("GOOG order book:");
    println!("  Imbalance:    {:.4}", snap.imbalance().unwrap_or(0.0));
    println!("  Weighted mid: {:.2}", snap.weighted_mid().unwrap_or(0.0));
    println!("  Mid price:    {:.2}", snap.mid_price().unwrap_or(0.0));
    println!("  Spread:       {} cents", snap.spread().unwrap_or(0));

    // === Final state ===
    println!("\n=== Final State ===\n");

    for (sym, bid, ask) in multi.best_prices() {
        let ex = multi.get(&sym).unwrap();
        println!(
            "  {sym}: bid={} ask={} trades={}",
            bid.map(|p| format!("{p}")).unwrap_or_else(|| "---".into()),
            ask.map(|p| format!("{p}")).unwrap_or_else(|| "---".into()),
            ex.trades().len(),
        );
    }
}
