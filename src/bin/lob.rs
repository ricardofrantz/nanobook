//! Interactive limit order book CLI.
//!
//! A REPL for experimenting with the order book.
//!
//! Usage:
//!   cargo run --bin lob
//!   lob  (if installed via cargo install)

use nanobook::{Exchange, OrderId, Price, Side, TimeInForce};
use std::io::{self, BufRead, Write};
#[cfg(feature = "persistence")]
use std::path::Path;

fn main() {
    let mut exchange = Exchange::new();

    println!("Limit Order Book CLI v0.2.0");
    println!("Type 'help' for commands, 'quit' to exit.\n");

    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        print!("lob> ");
        stdout.flush().unwrap();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap() == 0 {
            break; // EOF
        }

        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        let cmd = parts.first().map(|s| s.to_lowercase());

        match cmd.as_deref() {
            Some("help" | "h" | "?") => print_help(),
            Some("quit" | "exit" | "q") => break,
            Some("book" | "b") => print_book(&exchange),
            Some("trades" | "t") => print_trades(&exchange),
            Some("buy") => handle_order(&mut exchange, Side::Buy, &parts[1..]),
            Some("sell") => handle_order(&mut exchange, Side::Sell, &parts[1..]),
            Some("market") => handle_market(&mut exchange, &parts[1..]),
            Some("stop") => handle_stop(&mut exchange, &parts[1..]),
            Some("stoplimit") => handle_stop_limit(&mut exchange, &parts[1..]),
            Some("cancel" | "c") => handle_cancel(&mut exchange, &parts[1..]),
            #[cfg(feature = "persistence")]
            Some("save") => {
                if let Some(path) = parts.get(1) {
                    match exchange.save(Path::new(path)) {
                        Ok(()) => println!("Saved to {}", path),
                        Err(e) => println!("Save failed: {}", e),
                    }
                } else {
                    println!("Usage: save <path>");
                }
            }
            #[cfg(feature = "persistence")]
            Some("load") => {
                if let Some(path) = parts.get(1) {
                    match Exchange::load(Path::new(path)) {
                        Ok(ex) => {
                            exchange = ex;
                            println!("Loaded from {}", path);
                        }
                        Err(e) => println!("Load failed: {}", e),
                    }
                } else {
                    println!("Usage: load <path>");
                }
            }
            Some("clear") => {
                exchange = Exchange::new();
                println!("Book cleared.");
            }
            Some("status" | "s") => handle_status(&exchange, &parts[1..]),
            Some(cmd) => println!("Unknown command: '{}'. Type 'help' for commands.", cmd),
            None => {}
        }
    }

    println!("Goodbye!");
}

fn print_help() {
    println!(
        r#"
Commands:
  buy <price> <qty> [ioc|fok]        Submit buy limit order (default: GTC)
  sell <price> <qty> [ioc|fok]       Submit sell limit order (default: GTC)
  market <buy|sell> <qty>            Submit market order
  stop <buy|sell> <stop_price> <qty> Submit stop-market order
  stoplimit <buy|sell> <stop> <limit> <qty> [ioc|fok]
                                     Submit stop-limit order
  cancel <order_id>                  Cancel an order (regular or stop)
  status <order_id>                  Show order status
  book                               Show order book
  trades                             Show trade history
  clear                              Reset the exchange
  help                               Show this help
  quit                               Exit

Examples:
  buy 100.50 100                Buy 100 @ $100.50 GTC
  sell 101.00 50 ioc            Sell 50 @ $101.00 IOC
  market buy 200                Market buy 200 shares
  stop buy 105.00 100           Buy stop at $105.00 for 100
  stoplimit sell 95.00 94.50 100  Sell stop-limit: stop@95, limit@94.50
  cancel 3                      Cancel order #3

Prices are in dollars (e.g., 100.50 = $100.50)
"#
    );
}

fn print_book(exchange: &Exchange) {
    let snap = exchange.depth(10);

    println!();
    println!("            ORDER BOOK");
    println!("  ──────────────────────────────");

    if snap.asks.is_empty() && snap.bids.is_empty() {
        println!("  (empty)");
        println!();
        return;
    }

    // Asks (reversed - highest at top)
    for level in snap.asks.iter().rev() {
        let bar = "█".repeat((level.quantity / 50).min(20) as usize);
        println!(
            "  ASK ${:>8.2}  {:>6}  {:<20}  ({} orders)",
            level.price.0 as f64 / 100.0,
            level.quantity,
            bar,
            level.order_count
        );
    }

    // Spread
    match snap.spread() {
        Some(spread) => {
            println!("  ─────── spread: ${:.2} ───────", spread as f64 / 100.0);
        }
        None => {
            println!("  ─────── (no spread) ───────");
        }
    }

    // Bids
    for level in &snap.bids {
        let bar = "█".repeat((level.quantity / 50).min(20) as usize);
        println!(
            "  BID ${:>8.2}  {:>6}  {:<20}  ({} orders)",
            level.price.0 as f64 / 100.0,
            level.quantity,
            bar,
            level.order_count
        );
    }

    println!();
}

fn print_trades(exchange: &Exchange) {
    let trades = exchange.trades();

    if trades.is_empty() {
        println!("No trades yet.");
        return;
    }

    println!();
    println!("  TRADE HISTORY ({} trades)", trades.len());
    println!("  ──────────────────────────────────────────");
    println!(
        "  {:>4}  {:>10}  {:>8}  {:>6} → {:>6}",
        "ID", "Price", "Qty", "Buyer", "Seller"
    );

    for trade in trades.iter().rev().take(20) {
        let (buyer, seller) = match trade.aggressor_side {
            Side::Buy => (trade.aggressor_order_id.0, trade.passive_order_id.0),
            Side::Sell => (trade.passive_order_id.0, trade.aggressor_order_id.0),
        };
        println!(
            "  {:>4}  ${:>9.2}  {:>8}  {:>6} → {:>6}",
            trade.id.0,
            trade.price.0 as f64 / 100.0,
            trade.quantity,
            buyer,
            seller
        );
    }

    if trades.len() > 20 {
        println!("  ... and {} more", trades.len() - 20);
    }
    println!();
}

fn handle_order(exchange: &mut Exchange, side: Side, args: &[&str]) {
    if args.len() < 2 {
        println!(
            "Usage: {} <price> <qty> [ioc|fok]",
            side.to_string().to_lowercase()
        );
        return;
    }

    let Some(price) = parse_price(args[0]) else {
        println!("Invalid price: '{}'", args[0]);
        return;
    };

    let qty: u64 = match args[1].parse() {
        Ok(q) if q > 0 => q,
        _ => {
            println!("Invalid quantity: '{}'", args[1]);
            return;
        }
    };

    let tif = match args.get(2).map(|s| s.to_lowercase()).as_deref() {
        Some("ioc") => TimeInForce::IOC,
        Some("fok") => TimeInForce::FOK,
        Some("gtc") | None => TimeInForce::GTC,
        Some(other) => {
            println!("Unknown TIF: '{}'. Use gtc, ioc, or fok.", other);
            return;
        }
    };

    let result = exchange.submit_limit(side, price, qty, tif);

    println!(
        "Order #{}: {} {} @ ${:.2} {:?}",
        result.order_id.0,
        side,
        qty,
        price.0 as f64 / 100.0,
        tif
    );

    if !result.trades.is_empty() {
        println!("  Trades:");
        for trade in &result.trades {
            println!(
                "    {} @ ${:.2}",
                trade.quantity,
                trade.price.0 as f64 / 100.0
            );
        }
    }

    println!(
        "  Status: {:?} (filled: {}, resting: {}, cancelled: {})",
        result.status, result.filled_quantity, result.resting_quantity, result.cancelled_quantity
    );
}

fn handle_market(exchange: &mut Exchange, args: &[&str]) {
    if args.len() < 2 {
        println!("Usage: market <buy|sell> <qty>");
        return;
    }

    let side = match args[0].to_lowercase().as_str() {
        "buy" | "b" => Side::Buy,
        "sell" | "s" => Side::Sell,
        other => {
            println!("Invalid side: '{}'. Use buy or sell.", other);
            return;
        }
    };

    let qty: u64 = match args[1].parse() {
        Ok(q) if q > 0 => q,
        _ => {
            println!("Invalid quantity: '{}'", args[1]);
            return;
        }
    };

    let result = exchange.submit_market(side, qty);

    println!("Order #{}: MARKET {} {}", result.order_id.0, side, qty);

    if !result.trades.is_empty() {
        println!("  Trades:");
        for trade in &result.trades {
            println!(
                "    {} @ ${:.2}",
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
        println!("  Avg price: ${:.4}", avg);
    }

    println!(
        "  Status: {:?} (filled: {}, cancelled: {})",
        result.status, result.filled_quantity, result.cancelled_quantity
    );
}

fn handle_cancel(exchange: &mut Exchange, args: &[&str]) {
    if args.is_empty() {
        println!("Usage: cancel <order_id>");
        return;
    }

    let id: u64 = match args[0].parse() {
        Ok(i) => i,
        Err(_) => {
            println!("Invalid order ID: '{}'", args[0]);
            return;
        }
    };

    let result = exchange.cancel(OrderId(id));

    if result.success {
        println!(
            "Cancelled order #{} ({} shares)",
            id, result.cancelled_quantity
        );
    } else {
        println!("Failed to cancel order #{}: {:?}", id, result.error);
    }
}

fn handle_status(exchange: &Exchange, args: &[&str]) {
    if args.is_empty() {
        println!("Usage: status <order_id>");
        return;
    }

    let id: u64 = match args[0].parse() {
        Ok(i) => i,
        Err(_) => {
            println!("Invalid order ID: '{}'", args[0]);
            return;
        }
    };

    // Check stop book first
    if let Some(stop) = exchange.get_stop_order(OrderId(id)) {
        println!("Stop order #{}:", id);
        println!("  Side:       {:?}", stop.side);
        println!("  Stop price: ${:.2}", stop.stop_price.0 as f64 / 100.0);
        if let Some(lp) = stop.limit_price {
            println!("  Limit price: ${:.2}", lp.0 as f64 / 100.0);
        }
        println!("  Quantity:   {}", stop.quantity);
        println!("  Status:     {:?}", stop.status);
        println!("  TIF:        {:?}", stop.time_in_force);
        return;
    }

    match exchange.get_order(OrderId(id)) {
        Some(order) => {
            println!("Order #{}:", id);
            println!("  Side:      {:?}", order.side);
            println!("  Price:     ${:.2}", order.price.0 as f64 / 100.0);
            println!(
                "  Quantity:  {} (filled: {}, remaining: {})",
                order.original_quantity, order.filled_quantity, order.remaining_quantity
            );
            println!("  Status:    {:?}", order.status);
            println!("  TIF:       {:?}", order.time_in_force);
        }
        None => {
            println!("Order #{} not found", id);
        }
    }
}

fn handle_stop(exchange: &mut Exchange, args: &[&str]) {
    if args.len() < 3 {
        println!("Usage: stop <buy|sell> <stop_price> <qty>");
        return;
    }

    let side = match args[0].to_lowercase().as_str() {
        "buy" | "b" => Side::Buy,
        "sell" | "s" => Side::Sell,
        other => {
            println!("Invalid side: '{}'. Use buy or sell.", other);
            return;
        }
    };

    let Some(stop_price) = parse_price(args[1]) else {
        println!("Invalid stop price: '{}'", args[1]);
        return;
    };

    let qty: u64 = match args[2].parse() {
        Ok(q) if q > 0 => q,
        _ => {
            println!("Invalid quantity: '{}'", args[2]);
            return;
        }
    };

    let result = exchange.submit_stop_market(side, stop_price, qty);

    println!(
        "Stop order #{}: {} stop-market @ ${:.2} for {}",
        result.order_id.0,
        side,
        stop_price.0 as f64 / 100.0,
        qty
    );
    println!("  Status: {:?}", result.status);
}

fn handle_stop_limit(exchange: &mut Exchange, args: &[&str]) {
    if args.len() < 4 {
        println!("Usage: stoplimit <buy|sell> <stop_price> <limit_price> <qty> [ioc|fok]");
        return;
    }

    let side = match args[0].to_lowercase().as_str() {
        "buy" | "b" => Side::Buy,
        "sell" | "s" => Side::Sell,
        other => {
            println!("Invalid side: '{}'. Use buy or sell.", other);
            return;
        }
    };

    let Some(stop_price) = parse_price(args[1]) else {
        println!("Invalid stop price: '{}'", args[1]);
        return;
    };

    let Some(limit_price) = parse_price(args[2]) else {
        println!("Invalid limit price: '{}'", args[2]);
        return;
    };

    let qty: u64 = match args[3].parse() {
        Ok(q) if q > 0 => q,
        _ => {
            println!("Invalid quantity: '{}'", args[3]);
            return;
        }
    };

    let tif = match args.get(4).map(|s| s.to_lowercase()).as_deref() {
        Some("ioc") => TimeInForce::IOC,
        Some("fok") => TimeInForce::FOK,
        Some("gtc") | None => TimeInForce::GTC,
        Some(other) => {
            println!("Unknown TIF: '{}'. Use gtc, ioc, or fok.", other);
            return;
        }
    };

    let result = exchange.submit_stop_limit(side, stop_price, limit_price, qty, tif);

    println!(
        "Stop-limit order #{}: {} stop@${:.2} limit@${:.2} for {} {:?}",
        result.order_id.0,
        side,
        stop_price.0 as f64 / 100.0,
        limit_price.0 as f64 / 100.0,
        qty,
        tif
    );
    println!("  Status: {:?}", result.status);
}

fn parse_price(s: &str) -> Option<Price> {
    // Parse as float, convert to cents
    let f: f64 = s.parse().ok()?;
    if f <= 0.0 {
        return None;
    }
    Some(Price((f * 100.0).round() as i64))
}
