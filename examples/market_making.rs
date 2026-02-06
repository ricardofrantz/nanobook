// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Market making example: post two-sided quotes, react to fills, re-quote.
//!
//! Run with: cargo run --example market_making

use nanobook::{Exchange, OrderId, Price, Side, TimeInForce};

/// A simple market maker that posts symmetric quotes around a fair value.
struct MarketMaker {
    fair_value: i64,  // in cents
    half_spread: i64, // half-spread in cents
    quote_size: u64,
    bid_id: Option<OrderId>,
    ask_id: Option<OrderId>,
}

impl MarketMaker {
    fn new(fair_value: i64, half_spread: i64, quote_size: u64) -> Self {
        Self {
            fair_value,
            half_spread,
            quote_size,
            bid_id: None,
            ask_id: None,
        }
    }

    /// Cancel existing quotes and post new ones.
    fn requote(&mut self, exchange: &mut Exchange) {
        // Cancel old quotes
        if let Some(id) = self.bid_id.take() {
            exchange.cancel(id);
        }
        if let Some(id) = self.ask_id.take() {
            exchange.cancel(id);
        }

        // Post new quotes
        let bid_price = Price(self.fair_value - self.half_spread);
        let ask_price = Price(self.fair_value + self.half_spread);

        let bid = exchange.submit_limit(Side::Buy, bid_price, self.quote_size, TimeInForce::GTC);
        let ask = exchange.submit_limit(Side::Sell, ask_price, self.quote_size, TimeInForce::GTC);

        self.bid_id = Some(bid.order_id);
        self.ask_id = Some(ask.order_id);

        println!(
            "Quoted: BID {} @ {} | ASK {} @ {}",
            self.quote_size, bid_price, self.quote_size, ask_price
        );
    }

    /// Check if any quotes were hit and re-quote if so.
    fn check_fills(&mut self, exchange: &mut Exchange) -> bool {
        let mut filled = false;

        if let Some(id) = self.bid_id {
            if let Some(order) = exchange.get_order(id) {
                if order.filled_quantity > 0 {
                    println!(
                        "  Bid filled: {} @ {} (remaining: {})",
                        order.filled_quantity, order.price, order.remaining_quantity
                    );
                    filled = true;
                }
            }
        }

        if let Some(id) = self.ask_id {
            if let Some(order) = exchange.get_order(id) {
                if order.filled_quantity > 0 {
                    println!(
                        "  Ask filled: {} @ {} (remaining: {})",
                        order.filled_quantity, order.price, order.remaining_quantity
                    );
                    filled = true;
                }
            }
        }

        filled
    }
}

fn main() {
    let mut exchange = Exchange::new();
    let mut mm = MarketMaker::new(100_00, 50, 100); // Fair value $100, $0.50 spread, 100 shares

    println!("=== Market Making Simulation ===\n");

    // Round 1: Post initial quotes
    println!("Round 1: Initial quotes");
    mm.requote(&mut exchange);

    // Round 2: Aggressive buyer hits the ask
    println!("\nRound 2: Aggressive buyer");
    exchange.submit_market(Side::Buy, 50);
    if mm.check_fills(&mut exchange) {
        // Shift fair value up slightly after getting lifted
        mm.fair_value += 10;
        println!(
            "  Fair value adjusted to ${:.2}",
            mm.fair_value as f64 / 100.0
        );
        mm.requote(&mut exchange);
    }

    // Round 3: Aggressive seller hits the bid
    println!("\nRound 3: Aggressive seller");
    exchange.submit_market(Side::Sell, 75);
    if mm.check_fills(&mut exchange) {
        // Shift fair value down after getting hit
        mm.fair_value -= 15;
        println!(
            "  Fair value adjusted to ${:.2}",
            mm.fair_value as f64 / 100.0
        );
        mm.requote(&mut exchange);
    }

    // Round 4: Large sweep clears the ask completely
    println!("\nRound 4: Large sweep");
    exchange.submit_market(Side::Buy, 200);
    if mm.check_fills(&mut exchange) {
        mm.fair_value += 25;
        println!(
            "  Fair value adjusted to ${:.2}",
            mm.fair_value as f64 / 100.0
        );
        mm.requote(&mut exchange);
    }

    // Summary
    println!("\n=== Summary ===");
    println!("Total trades: {}", exchange.trades().len());
    println!(
        "Final book: BID {:?} | ASK {:?}",
        exchange.best_bid(),
        exchange.best_ask()
    );

    let mut pnl: i64 = 0;
    for trade in exchange.trades() {
        // MM is always the passive side
        let mm_side = trade.passive_side();
        let sign = match mm_side {
            Side::Buy => -1, // bought = spent money
            Side::Sell => 1, // sold = received money
        };
        pnl += sign * trade.price.0 * trade.quantity as i64;
    }
    println!("Realized PnL: {} cents", pnl);
}
