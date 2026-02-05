//! Quick non-interactive demo (no pauses) - good for recording GIFs.
//!
//! Run with: cargo run --example demo_quick

use limit_order_book::{Exchange, Price, Side, TimeInForce};

fn main() {
    println!("\n{}", "=".repeat(60));
    println!("       LIMIT ORDER BOOK - MATCHING DEMO");
    println!("{}\n", "=".repeat(60));

    let mut exchange = Exchange::new();

    // Build the book
    println!("Building order book...\n");

    println!("  SELL 100 @ $50.25 (Alice)");
    exchange.submit_limit(Side::Sell, Price(50_25), 100, TimeInForce::GTC);

    println!("  SELL 150 @ $50.50 (Bob)");
    exchange.submit_limit(Side::Sell, Price(50_50), 150, TimeInForce::GTC);

    println!("  BUY  100 @ $50.00 (Carol)");
    exchange.submit_limit(Side::Buy, Price(50_00), 100, TimeInForce::GTC);

    println!("  BUY  200 @ $49.75 (Dan)");
    exchange.submit_limit(Side::Buy, Price(49_75), 200, TimeInForce::GTC);

    print_book(&exchange, "Initial Book");

    // Crossing order
    println!("\nIncoming: BUY 120 @ $50.25 (Eve) - CROSSES SPREAD!\n");
    let result = exchange.submit_limit(Side::Buy, Price(50_25), 120, TimeInForce::GTC);

    println!("  Trades:");
    for trade in &result.trades {
        println!(
            "    {} shares @ ${:.2}",
            trade.quantity,
            trade.price.0 as f64 / 100.0
        );
    }
    println!(
        "  Filled: {}, Resting: {}",
        result.filled_quantity, result.resting_quantity
    );

    print_book(&exchange, "After Matching");

    // Market sweep
    println!("\nIncoming: MARKET BUY 200 shares (Frank)\n");
    let result = exchange.submit_market(Side::Buy, 200);

    println!("  Trades:");
    for trade in &result.trades {
        println!(
            "    {} shares @ ${:.2}",
            trade.quantity,
            trade.price.0 as f64 / 100.0
        );
    }

    let avg = result
        .trades
        .iter()
        .map(|t| t.price.0 as f64 * t.quantity as f64)
        .sum::<f64>()
        / result.filled_quantity as f64
        / 100.0;
    println!("  Avg price: ${:.4} (slippage!)", avg);

    print_book(&exchange, "Final Book");

    println!("\nTotal trades executed: {}", exchange.trades().len());
    println!("{}\n", "=".repeat(60));
}

fn print_book(exchange: &Exchange, title: &str) {
    let snap = exchange.depth(5);

    println!("\n  {}", title);
    println!("  {}", "-".repeat(40));

    // Asks (reversed for display)
    for level in snap.asks.iter().rev() {
        println!(
            "  ASK  ${:>6.2}  {:>4} shares",
            level.price.0 as f64 / 100.0,
            level.quantity
        );
    }

    // Spread
    if let Some(spread) = snap.spread() {
        println!("  ---- spread: ${:.2} ----", spread as f64 / 100.0);
    } else {
        println!("  ---- (no spread) ----");
    }

    // Bids
    for level in &snap.bids {
        println!(
            "  BID  ${:>6.2}  {:>4} shares",
            level.price.0 as f64 / 100.0,
            level.quantity
        );
    }
}
