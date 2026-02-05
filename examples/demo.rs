//! Interactive demo showing how the limit order book works.
//!
//! Run with: cargo run --example demo

use limit_order_book::{Exchange, OrderId, Price, Side, TimeInForce};

fn main() {
    println!("\n{}", "=".repeat(70));
    println!("              LIMIT ORDER BOOK - INTERACTIVE DEMO");
    println!("{}\n", "=".repeat(70));

    let mut exchange = Exchange::new();

    // ─────────────────────────────────────────────────────────────────────
    // PHASE 1: Building the order book
    // ─────────────────────────────────────────────────────────────────────

    section("PHASE 1: Building the Order Book");

    explain(
        "An order book has two sides:
  • BIDS (buy orders)  - sorted highest price first (buyers want the best deal)
  • ASKS (sell orders) - sorted lowest price first (sellers want the best deal)

The gap between best bid and best ask is called the 'spread'.
Let's add some orders to build a market for ACME stock...",
    );

    pause();

    // Add asks (sellers)
    step("Seller Alice posts: SELL 100 @ $50.25");
    let alice = exchange.submit_limit(Side::Sell, Price(50_25), 100, TimeInForce::GTC);
    print_book(&exchange);

    step("Seller Bob posts: SELL 150 @ $50.50");
    let _bob = exchange.submit_limit(Side::Sell, Price(50_50), 150, TimeInForce::GTC);
    print_book(&exchange);

    step("Seller Carol posts: SELL 200 @ $50.25 (same price as Alice)");
    let carol = exchange.submit_limit(Side::Sell, Price(50_25), 200, TimeInForce::GTC);
    print_book(&exchange);

    explain(
        "Notice Alice and Carol are at the same price ($50.25).
Alice was first, so she has TIME PRIORITY. Her order will fill before Carol's.",
    );

    pause();

    // Add bids (buyers)
    step("Buyer Dan posts: BUY 100 @ $50.00");
    exchange.submit_limit(Side::Buy, Price(50_00), 100, TimeInForce::GTC);
    print_book(&exchange);

    step("Buyer Eve posts: BUY 200 @ $49.75");
    exchange.submit_limit(Side::Buy, Price(49_75), 200, TimeInForce::GTC);
    print_book(&exchange);

    step("Buyer Frank posts: BUY 50 @ $50.00 (same price as Dan)");
    exchange.submit_limit(Side::Buy, Price(50_00), 50, TimeInForce::GTC);
    print_book(&exchange);

    explain(
        "The spread is $0.25 (best ask $50.25 - best bid $50.00).
No trades yet because no orders 'cross' - buyers aren't willing to pay
what sellers are asking.",
    );

    // ─────────────────────────────────────────────────────────────────────
    // PHASE 2: A crossing order triggers matching
    // ─────────────────────────────────────────────────────────────────────

    section("PHASE 2: Crossing the Spread");

    explain(
        "Now a new buyer arrives willing to pay $50.25 - the current best ask.
This 'crosses' the spread and triggers matching!",
    );

    pause();

    step("Buyer George posts: BUY 150 @ $50.25 (crosses the spread!)");
    let result = exchange.submit_limit(Side::Buy, Price(50_25), 150, TimeInForce::GTC);

    println!("\n  ⚡ TRADES EXECUTED:");
    for trade in &result.trades {
        println!(
            "     {} shares @ ${:.2} (buyer #{} ← seller #{})",
            trade.quantity,
            trade.price.0 as f64 / 100.0,
            trade.aggressor_order_id.0,
            trade.passive_order_id.0
        );
    }
    println!(
        "\n  📊 George filled {} shares, {} remaining on book",
        result.filled_quantity, result.resting_quantity
    );

    print_book(&exchange);

    explain(&format!(
        "George bought 150 shares:
  • First 100 from Alice (she had time priority at $50.25)
  • Then 50 from Carol (next in line at $50.25)

Alice's order #{} is completely filled and removed.
Carol's order #{} is partially filled: 200 → 150 remaining.",
        alice.order_id.0, carol.order_id.0
    ));

    // ─────────────────────────────────────────────────────────────────────
    // PHASE 3: Market order sweeps multiple levels
    // ─────────────────────────────────────────────────────────────────────

    section("PHASE 3: Market Order Sweep");

    explain(
        "A MARKET order executes immediately at the best available prices.
It can 'sweep' through multiple price levels if needed.

Watch what happens when someone wants to buy 250 shares NOW...",
    );

    pause();

    step("Buyer Helen submits: MARKET BUY 250 shares");
    let result = exchange.submit_market(Side::Buy, 250);

    println!("\n  ⚡ TRADES EXECUTED:");
    for trade in &result.trades {
        println!(
            "     {} shares @ ${:.2}",
            trade.quantity,
            trade.price.0 as f64 / 100.0
        );
    }

    let avg_price: f64 = result
        .trades
        .iter()
        .map(|t| t.price.0 as f64 * t.quantity as f64)
        .sum::<f64>()
        / result.filled_quantity as f64
        / 100.0;

    println!(
        "\n  📊 Helen filled {} shares @ avg ${:.4}",
        result.filled_quantity, avg_price
    );
    if result.cancelled_quantity > 0 {
        println!(
            "     {} shares cancelled (not enough liquidity)",
            result.cancelled_quantity
        );
    }

    print_book(&exchange);

    explain(
        "Helen's market order swept through:
  1. Carol's remaining 150 @ $50.25
  2. Bob's 100 @ $50.50 (partial fill)

She paid more for the second batch - this is called 'slippage'.
Bob still has 50 shares resting at $50.50.",
    );

    // ─────────────────────────────────────────────────────────────────────
    // PHASE 4: IOC and FOK
    // ─────────────────────────────────────────────────────────────────────

    section("PHASE 4: Time-in-Force (IOC vs FOK)");

    explain(
        "Orders have a Time-in-Force that controls their behavior:

  • GTC (Good-Til-Cancelled): Rest on book until filled or cancelled
  • IOC (Immediate-or-Cancel): Fill what you can, cancel the rest
  • FOK (Fill-or-Kill): Fill entirely or cancel entirely

Let's see the difference...",
    );

    pause();

    step("Ivan tries: IOC BUY 100 @ $50.50");
    let result = exchange.submit_limit(Side::Buy, Price(50_50), 100, TimeInForce::IOC);
    println!(
        "  → Filled: {}, Cancelled: {} (IOC fills what's available)",
        result.filled_quantity, result.cancelled_quantity
    );
    print_book(&exchange);

    step("Julia tries: FOK BUY 100 @ $50.50 (but only 0 available!)");
    let result = exchange.submit_limit(Side::Buy, Price(50_50), 100, TimeInForce::FOK);
    println!(
        "  → Filled: {}, Cancelled: {} (FOK: all or nothing!)",
        result.filled_quantity, result.cancelled_quantity
    );
    print_book(&exchange);

    // ─────────────────────────────────────────────────────────────────────
    // PHASE 5: Order cancellation
    // ─────────────────────────────────────────────────────────────────────

    section("PHASE 5: Order Cancellation");

    explain("Resting orders can be cancelled at any time.");

    pause();

    // Find Dan's order (first bid at $50.00)
    let dan_order_id = OrderId(4); // Dan was order #4

    step(&format!("Dan cancels his order #{}", dan_order_id.0));
    let result = exchange.cancel(dan_order_id);
    println!("  → Cancelled {} shares", result.cancelled_quantity);
    print_book(&exchange);

    // ─────────────────────────────────────────────────────────────────────
    // Summary
    // ─────────────────────────────────────────────────────────────────────

    section("SUMMARY");

    let snapshot = exchange.depth(10);
    println!("  Final book state:");
    println!(
        "    Best Bid: {}",
        snapshot
            .best_bid()
            .map(|p| format!("${:.2}", p.0 as f64 / 100.0))
            .unwrap_or_else(|| "None".to_string())
    );
    println!(
        "    Best Ask: {}",
        snapshot
            .best_ask()
            .map(|p| format!("${:.2}", p.0 as f64 / 100.0))
            .unwrap_or_else(|| "None".to_string())
    );
    println!(
        "    Spread:   {}",
        snapshot
            .spread()
            .map(|s| format!("${:.2}", s as f64 / 100.0))
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!("    Trades:   {}", exchange.trades().len());

    println!("\n  Key concepts demonstrated:");
    println!("    ✓ Price-time priority (FIFO at each price level)");
    println!("    ✓ Crossing orders trigger immediate matching");
    println!("    ✓ Market orders sweep through multiple levels");
    println!("    ✓ IOC fills what it can, cancels the rest");
    println!("    ✓ FOK requires full fill or cancels entirely");
    println!("    ✓ Orders can be cancelled while resting");

    println!("\n{}", "=".repeat(70));
    println!("  For more, see: https://github.com/ricardofrantz/limit-order-book-rs");
    println!("{}\n", "=".repeat(70));
}

// ═══════════════════════════════════════════════════════════════════════════
// Helper functions
// ═══════════════════════════════════════════════════════════════════════════

fn print_book(exchange: &Exchange) {
    let snapshot = exchange.depth(5);

    println!();
    println!("  ┌─────────────────────────────────────────────────────────────┐");
    println!("  │                      ORDER BOOK                             │");
    println!("  ├─────────────────────────────────────────────────────────────┤");

    // Print asks in reverse (highest first for display, but we want lowest at bottom)
    let asks: Vec<_> = snapshot.asks.iter().rev().collect();
    if asks.is_empty() {
        println!("  │  ASKS:  (empty)                                            │");
    } else {
        for (i, level) in asks.iter().enumerate() {
            let bar = "█".repeat((level.quantity / 20).min(20) as usize);
            let label = if i == asks.len() - 1 { "→" } else { " " };
            println!(
                "  │  {} ${:>6.2}  {:>4} │{:<20}│ {:>2} orders           │",
                label,
                level.price.0 as f64 / 100.0,
                level.quantity,
                bar,
                level.order_count
            );
        }
    }

    // Spread line
    let spread = snapshot.spread().map(|s| s as f64 / 100.0);
    println!("  ├─────────────────────────────────────────────────────────────┤");
    println!(
        "  │  SPREAD: {}                                          │",
        spread
            .map(|s| format!("${:.2}", s))
            .unwrap_or_else(|| "N/A".to_string())
    );
    println!("  ├─────────────────────────────────────────────────────────────┤");

    // Print bids (highest first)
    if snapshot.bids.is_empty() {
        println!("  │  BIDS:  (empty)                                            │");
    } else {
        for (i, level) in snapshot.bids.iter().enumerate() {
            let bar = "█".repeat((level.quantity / 20).min(20) as usize);
            let label = if i == 0 { "→" } else { " " };
            println!(
                "  │  {} ${:>6.2}  {:>4} │{:<20}│ {:>2} orders           │",
                label,
                level.price.0 as f64 / 100.0,
                level.quantity,
                bar,
                level.order_count
            );
        }
    }

    println!("  └─────────────────────────────────────────────────────────────┘");
    println!();
}

fn section(title: &str) {
    println!("\n");
    println!("  ╔═══════════════════════════════════════════════════════════════╗");
    println!("  ║  {:<61} ║", title);
    println!("  ╚═══════════════════════════════════════════════════════════════╝");
    println!();
}

fn step(description: &str) {
    println!("  ▶ {}", description);
}

fn explain(text: &str) {
    println!();
    for line in text.lines() {
        println!("  {}", line);
    }
    println!();
}

fn pause() {
    println!("  [Press Enter to continue...]");
    let mut input = String::new();
    std::io::stdin().read_line(&mut input).ok();
}
