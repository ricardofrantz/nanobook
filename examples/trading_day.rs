// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Comprehensive trading day demo: exercises every nanobook capability
//! through a realistic "day in the life" of a small quant fund.
//!
//! Run with: cargo run --features "portfolio,persistence" --example trading_day

#[cfg(all(feature = "portfolio", feature = "persistence"))]
fn main() {
    use std::path::Path;

    use nanobook::portfolio::{CostModel, Portfolio, Strategy, run_backtest};
    use nanobook::{
        Exchange, MultiExchange, Price, Side, Symbol, TimeInForce, Trade, TrailMethod,
    };

    let aapl = Symbol::new("AAPL");
    let msft = Symbol::new("MSFT");
    let goog = Symbol::new("GOOG");

    let mut multi = MultiExchange::new();

    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║         nanobook — Comprehensive Trading Day Demo           ║");
    println!("║       Small quant fund trading AAPL, MSFT, GOOG            ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    // =====================================================================
    // Phase 1: Pre-Market — Build Initial Liquidity
    // =====================================================================
    print_section("Phase 1: Pre-Market — Build Initial Liquidity");

    // AAPL: mid ~$150
    let aapl_ex = multi.get_or_create(&aapl);
    for i in 0i64..5 {
        aapl_ex.submit_limit(Side::Buy, Price(149_50 - i * 25), (100 + i * 20) as u64, TimeInForce::GTC);
        aapl_ex.submit_limit(Side::Sell, Price(150_50 + i * 25), (100 + i * 20) as u64, TimeInForce::GTC);
    }

    // MSFT: mid ~$300
    let msft_ex = multi.get_or_create(&msft);
    for i in 0i64..5 {
        msft_ex.submit_limit(Side::Buy, Price(299_50 - i * 50), (50 + i * 10) as u64, TimeInForce::GTC);
        msft_ex.submit_limit(Side::Sell, Price(300_50 + i * 50), (50 + i * 10) as u64, TimeInForce::GTC);
    }

    // GOOG: mid ~$140
    let goog_ex = multi.get_or_create(&goog);
    for i in 0i64..5 {
        goog_ex.submit_limit(Side::Buy, Price(139_50 - i * 25), (120 + i * 25) as u64, TimeInForce::GTC);
        goog_ex.submit_limit(Side::Sell, Price(140_50 + i * 25), (120 + i * 25) as u64, TimeInForce::GTC);
    }

    // Display BBO via best_prices()
    println!("  Best prices across all symbols:");
    for (sym, bid, ask) in multi.best_prices() {
        println!(
            "    {sym}: bid={} ask={}",
            bid.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
            ask.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
        );
    }

    // Show L2 depth for AAPL
    let snap = multi.get(&aapl).unwrap().depth(5);
    println!("\n  AAPL L2 depth (top 5):");
    print_book_snap("    ", &snap);

    // =====================================================================
    // Phase 2: Market Open — Aggressive Trading
    // =====================================================================
    print_section("Phase 2: Market Open — Aggressive Trading");

    // Market buy 400 AAPL — sweeps multiple ask levels
    let aapl_ex = multi.get_or_create(&aapl);
    let result = aapl_ex.submit_market(Side::Buy, 400);
    println!(
        "  Market BUY 400 AAPL: filled {} across {} trades",
        result.filled_quantity,
        result.trades.len()
    );
    for t in &result.trades {
        println!("    {} shares @ {}", t.quantity, t.price);
    }
    let vwap = Trade::vwap(&result.trades);
    println!("  VWAP: {}", vwap.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()));

    // IOC limit buy on GOOG — partial fill, remainder cancelled
    let goog_ex = multi.get_or_create(&goog);
    let result = goog_ex.submit_limit(Side::Buy, Price(140_75), 200, TimeInForce::IOC);
    println!(
        "\n  IOC BUY 200 GOOG @ $140.75: filled={}, cancelled={}",
        result.filled_quantity, result.cancelled_quantity
    );
    if !result.trades.is_empty() {
        let vwap = Trade::vwap(&result.trades).unwrap();
        println!("    VWAP: {vwap}");
    }

    // =====================================================================
    // Phase 3: FOK & Order Types
    // =====================================================================
    print_section("Phase 3: FOK — Fill-or-Kill Rejection");

    let msft_ex = multi.get_or_create(&msft);
    let bbo_before = msft_ex.best_bid_ask();
    let result = msft_ex.submit_limit(Side::Buy, Price(300_50), 5000, TimeInForce::FOK);
    let bbo_after = msft_ex.best_bid_ask();
    println!(
        "  FOK BUY 5000 MSFT @ $300.50: status={:?}, filled={}",
        result.status, result.filled_quantity
    );
    println!(
        "  Book untouched? {} (BBO: {:?} → {:?})",
        bbo_before == bbo_after,
        bbo_before,
        bbo_after
    );

    // =====================================================================
    // Phase 4: Portfolio Rebalancing via LOB
    // =====================================================================
    print_section("Phase 4: Portfolio Rebalancing via LOB");

    let cost_model = CostModel {
        commission_bps: 3,
        slippage_bps: 2,
        min_trade_fee: 1_00,
    };
    let mut portfolio = Portfolio::new(1_000_000_00, cost_model);

    println!("  Initial cash: $1,000,000.00");
    println!("  Cost model: 3 bps commission + 2 bps slippage + $1.00 min fee");

    // Replenish liquidity for LOB rebalancing (the market buys consumed some)
    let aapl_ex = multi.get_or_create(&aapl);
    for i in 0i64..10 {
        aapl_ex.submit_limit(Side::Sell, Price(150_00 + i * 10), 500, TimeInForce::GTC);
        aapl_ex.submit_limit(Side::Buy, Price(149_90 - i * 10), 500, TimeInForce::GTC);
    }
    let msft_ex = multi.get_or_create(&msft);
    for i in 0i64..10 {
        msft_ex.submit_limit(Side::Sell, Price(300_00 + i * 20), 300, TimeInForce::GTC);
        msft_ex.submit_limit(Side::Buy, Price(299_80 - i * 20), 300, TimeInForce::GTC);
    }
    let goog_ex = multi.get_or_create(&goog);
    for i in 0i64..10 {
        goog_ex.submit_limit(Side::Sell, Price(140_00 + i * 10), 500, TimeInForce::GTC);
        goog_ex.submit_limit(Side::Buy, Price(139_90 - i * 10), 500, TimeInForce::GTC);
    }

    // Target: 50% AAPL, 30% MSFT, 20% GOOG
    let targets = [(aapl, 0.5), (msft, 0.3), (goog, 0.2)];
    portfolio.rebalance_lob(&targets, &mut multi);

    println!("\n  Target allocation: 50% AAPL / 30% MSFT / 20% GOOG");
    println!("  Positions after LOB rebalancing:");
    let prices: Vec<(Symbol, i64)> = vec![(aapl, 150_00), (msft, 300_00), (goog, 140_00)];
    for (sym, pos) in portfolio.positions() {
        let price = prices.iter().find(|(s, _)| s == sym).map(|(_, p)| *p).unwrap_or(0);
        println!(
            "    {sym}: qty={:>6}, avg_entry={}, mkt_value=${:.2}, unrealized_pnl=${:.2}",
            pos.quantity,
            cents(pos.avg_entry_price),
            pos.market_value(price) as f64 / 100.0,
            pos.unrealized_pnl(price) as f64 / 100.0,
        );
    }

    let snap = portfolio.snapshot(&prices);
    println!(
        "  Portfolio: equity=${:.2}, cash=${:.2}, positions={}",
        snap.equity as f64 / 100.0,
        snap.cash as f64 / 100.0,
        snap.num_positions
    );

    // =====================================================================
    // Phase 5: Risk Management — Stop Orders
    // =====================================================================
    print_section("Phase 5: Risk Management — Stop Orders");

    // 5a. Sell stop-market on AAPL @ $146.00 (protective stop-loss)
    let aapl_ex = multi.get_or_create(&aapl);
    let stop1 = aapl_ex.submit_stop_market(Side::Sell, Price(146_00), 200);
    println!(
        "  [a] Sell stop-market AAPL @ $146.00 (200 shares): id={}, status={:?}",
        stop1.order_id, stop1.status
    );

    // 5b. Sell stop-limit on MSFT @ stop=$295.00, limit=$294.50
    let msft_ex = multi.get_or_create(&msft);
    let stop2 = msft_ex.submit_stop_limit(
        Side::Sell,
        Price(295_00),
        Price(294_50),
        100,
        TimeInForce::GTC,
    );
    println!(
        "  [b] Sell stop-limit MSFT @ stop=$295.00 / limit=$294.50 (100 shares): id={}, status={:?}",
        stop2.order_id, stop2.status
    );

    // 5c. Sell trailing stop-market on GOOG, Fixed($2.00) trail
    let goog_ex = multi.get_or_create(&goog);
    let stop3 = goog_ex.submit_trailing_stop_market(
        Side::Sell,
        Price(138_00),
        150,
        TrailMethod::Fixed(2_00),
    );
    println!(
        "  [c] Sell trailing stop GOOG, Fixed($2.00), init=$138.00 (150 shares): id={}, status={:?}",
        stop3.order_id, stop3.status
    );

    // 5d. Sell trailing stop with Percentage(5%)
    let goog_ex = multi.get_or_create(&goog);
    let stop4 = goog_ex.submit_trailing_stop_market(
        Side::Sell,
        Price(133_00),
        100,
        TrailMethod::Percentage(0.05),
    );
    println!(
        "  [d] Sell trailing stop GOOG, Pct(5%), init=$133.00 (100 shares): id={}, status={:?}",
        stop4.order_id, stop4.status
    );

    // 5e. Cancel the percentage trailing stop
    let goog_ex = multi.get_or_create(&goog);
    let cancel = goog_ex.cancel(stop4.order_id);
    println!(
        "  [e] Cancel trailing stop {}: success={}, pending_stops={}",
        stop4.order_id,
        cancel.success,
        goog_ex.pending_stop_count()
    );

    // 5f. Modify AAPL stop (cancel+replace) — loses time priority
    let aapl_ex = multi.get_or_create(&aapl);
    let old_stop_id = stop1.order_id;
    let cancel = aapl_ex.cancel(old_stop_id);
    let new_stop = aapl_ex.submit_stop_market(Side::Sell, Price(147_00), 200);
    println!(
        "  [f] Modify AAPL stop: cancelled {} (qty={}), new stop {} @ $147.00",
        old_stop_id, cancel.cancelled_quantity, new_stop.order_id
    );
    println!(
        "      New ID > old ID ({} > {}) — lost time priority",
        new_stop.order_id, old_stop_id
    );

    println!("\n  Pending stops: AAPL={}, MSFT={}, GOOG={}",
        multi.get(&aapl).unwrap().pending_stop_count(),
        multi.get(&msft).unwrap().pending_stop_count(),
        multi.get(&goog).unwrap().pending_stop_count(),
    );

    // =====================================================================
    // Phase 6: Market Movement — Trailing Stop Adjustment
    // =====================================================================
    print_section("Phase 6: Market Movement — Trailing Stop Adjustment");

    // Simulate GOOG rally by sweeping resting asks upward.
    // Each market buy fills at progressively higher prices,
    // pushing last_trade_price up and dragging the trailing stop along.
    let goog_ex = multi.get_or_create(&goog);
    let rally_prices = [141_00i64, 142_00, 143_00];
    for &target in &rally_prices {
        // Market buy to sweep through the ask book up to this price
        let result = goog_ex.submit_limit(Side::Buy, Price(target), 500, TimeInForce::IOC);
        let last = goog_ex.last_trade_price().unwrap();
        println!(
            "  GOOG aggressive buy to {}: filled={}, last_trade={}",
            cents(target),
            result.filled_quantity,
            last,
        );
        if let Some(stop) = goog_ex.get_stop_order(stop3.order_id) {
            println!(
                "    Trailing stop: watermark={}, stop_price={}",
                stop.watermark.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
                stop.stop_price,
            );
        }
    }

    // Book analytics on GOOG
    let goog_snap = multi.get(&goog).unwrap().depth(5);
    println!("\n  GOOG book analytics:");
    if let Some(imb) = goog_snap.imbalance() {
        println!("    Imbalance:    {imb:>+.4}");
    }
    if let Some(wmid) = goog_snap.weighted_mid() {
        println!("    Weighted mid: {}", cents(wmid as i64));
    }
    if let Some(mid) = goog_snap.mid_price() {
        println!("    Mid price:    {}", cents(mid as i64));
    }
    if let Some(spread) = goog_snap.spread() {
        println!("    Spread:       {}", cents(spread));
    }

    // L3 full book snapshot
    let full = multi.get(&goog).unwrap().full_book();
    println!(
        "\n  GOOG L3 full book: {} bid levels, {} ask levels",
        full.bids.len(),
        full.asks.len()
    );

    // =====================================================================
    // Phase 7: Flash Dip — Stop Cascade
    // =====================================================================
    print_section("Phase 7: Flash Dip — Stop Cascade");

    // Add bids at and below $147 that the stop-loss will eventually fill against
    let aapl_ex = multi.get_or_create(&aapl);
    aapl_ex.submit_limit(Side::Buy, Price(147_50), 100, TimeInForce::GTC);
    aapl_ex.submit_limit(Side::Buy, Price(147_00), 100, TimeInForce::GTC);
    aapl_ex.submit_limit(Side::Buy, Price(146_50), 100, TimeInForce::GTC);
    aapl_ex.submit_limit(Side::Buy, Price(146_00), 200, TimeInForce::GTC);
    aapl_ex.submit_limit(Side::Buy, Price(145_00), 300, TimeInForce::GTC);
    aapl_ex.submit_limit(Side::Buy, Price(144_00), 400, TimeInForce::GTC);

    let aapl_ex = multi.get_or_create(&aapl);
    let trades_before = aapl_ex.trades().len();
    let stops_before = aapl_ex.pending_stop_count();
    println!("  Before: {} trades, {} pending stops", trades_before, stops_before);
    println!(
        "  AAPL BBO: bid={} ask={}",
        aapl_ex.best_bid().map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
        aapl_ex.best_ask().map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
    );

    // Massive limit sell at $146.50 sweeps all bids down to that level,
    // crashing last_trade through the $147.00 stop price → triggers cascade
    let aapl_ex = multi.get_or_create(&aapl);
    let result = aapl_ex.submit_limit(Side::Sell, Price(146_50), 10000, TimeInForce::IOC);
    let trades_after = aapl_ex.trades().len();
    let stops_after = aapl_ex.pending_stop_count();
    let new_trades = trades_after - trades_before;

    println!(
        "  Aggressive SELL 10000 @ $146.50: filled={}, last_trade={}",
        result.filled_quantity,
        aapl_ex.last_trade_price().map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
    );
    println!(
        "  After: {} trades (+{}), {} pending stops (was {})",
        trades_after, new_trades, stops_after, stops_before
    );

    if stops_after < stops_before {
        println!(
            "  >>> Stop cascade! Sell stop at $147.00 triggered → market sell 200 shares"
        );
    }

    // Show L1 during volatility
    let aapl_ex = multi.get(&aapl).unwrap();
    let (bid, ask) = aapl_ex.best_bid_ask();
    println!(
        "  AAPL L1 after flash dip: bid={} ask={}",
        bid.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
        ask.map(|p| format!("{p}")).unwrap_or_else(|| "—".into()),
    );
    println!(
        "  Last trade price: {}",
        aapl_ex
            .last_trade_price()
            .map(|p| format!("{p}"))
            .unwrap_or_else(|| "—".into())
    );
    println!(
        "  Trade history length: {}",
        aapl_ex.trades().len()
    );

    // =====================================================================
    // Phase 8: Strategy Backtest (SimpleFill)
    // =====================================================================
    print_section("Phase 8: Strategy Backtest (SimpleFill)");

    // A simple momentum strategy
    struct MomentumStrategy;

    impl Strategy for MomentumStrategy {
        fn compute_weights(
            &self,
            bar_index: usize,
            prices: &[(Symbol, i64)],
            _portfolio: &Portfolio,
        ) -> Vec<(Symbol, f64)> {
            if bar_index == 0 || prices.is_empty() {
                // First bar: equal weight to establish positions
                let n = prices.len() as f64;
                return prices.iter().map(|&(sym, _)| (sym, 1.0 / n)).collect();
            }
            // Simple momentum: overweight symbols with higher prices
            // (in real life you'd compare to prior bar)
            let total: f64 = prices.iter().map(|(_, p)| *p as f64).sum();
            prices
                .iter()
                .map(|&(sym, p)| (sym, p as f64 / total))
                .collect()
        }
    }

    // Synthetic 12-month price series
    let price_series: Vec<Vec<(Symbol, i64)>> = vec![
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

    let bt = run_backtest(
        &MomentumStrategy,
        &price_series,
        1_000_000_00,
        CostModel::zero(),
        12.0,  // monthly periods
        0.04 / 12.0, // ~4% annual risk-free
    );

    println!("  Momentum strategy backtest over 12 months:");
    println!("  Initial capital: $1,000,000");
    println!("  Returns recorded: {}", bt.portfolio.returns().len());
    println!("  Equity curve points: {}", bt.portfolio.equity_curve().len());

    if let Some(ref m) = bt.metrics {
        println!("\n{m}");
    }

    // =====================================================================
    // Phase 9: Memory Management
    // =====================================================================
    print_section("Phase 9: Memory Management");

    // compact() on AAPL — remove tombstones from cancelled orders
    let aapl_ex = multi.get_or_create(&aapl);
    let orders_before = aapl_ex.book().order_count();
    aapl_ex.compact();
    let orders_after = aapl_ex.book().order_count();
    println!(
        "  AAPL compact(): active orders {} → {} (tombstones removed)",
        orders_before, orders_after
    );

    // clear_trades() — free trade history
    let aapl_ex = multi.get_or_create(&aapl);
    let trade_count = aapl_ex.trades().len();
    aapl_ex.clear_trades();
    println!(
        "  AAPL clear_trades(): {} trades freed, now {}",
        trade_count,
        aapl_ex.trades().len()
    );

    // clear_order_history() — remove filled/cancelled orders, keep active
    let aapl_ex = multi.get_or_create(&aapl);
    let removed = aapl_ex.clear_order_history();
    println!(
        "  AAPL clear_order_history(): {} inactive orders removed",
        removed
    );

    // =====================================================================
    // Phase 10: Event Sourcing & Persistence
    // =====================================================================
    print_section("Phase 10: Event Sourcing & Persistence");

    // Count events by type on MSFT (which wasn't compacted)
    let msft_ex = multi.get(&msft).unwrap();
    let events = msft_ex.events();
    let mut limit_count = 0;
    let mut market_count = 0;
    let mut cancel_count = 0;
    let mut stop_count = 0;
    let mut other_count = 0;
    for event in events {
        match event {
            nanobook::Event::SubmitLimit { .. } => limit_count += 1,
            nanobook::Event::SubmitMarket { .. } => market_count += 1,
            nanobook::Event::Cancel { .. } => cancel_count += 1,
            nanobook::Event::SubmitStopMarket { .. }
            | nanobook::Event::SubmitStopLimit { .. } => stop_count += 1,
            _ => other_count += 1,
        }
    }
    println!("  MSFT event log: {} total events", events.len());
    println!(
        "    Limits={limit_count}, Markets={market_count}, Cancels={cancel_count}, Stops={stop_count}, Other={other_count}"
    );

    // Save MSFT exchange to file
    let save_path = Path::new("trading_day_msft.jsonl");
    let msft_ex = multi.get(&msft).unwrap();
    let orig_bbo = msft_ex.best_bid_ask();
    let orig_trade_count = msft_ex.trades().len();
    let orig_stop_count = msft_ex.pending_stop_count();

    msft_ex.save(save_path).expect("failed to save");
    println!("\n  Saved MSFT to {}", save_path.display());

    // Load from file
    let loaded = Exchange::load(save_path).expect("failed to load");
    let loaded_bbo = loaded.best_bid_ask();
    let loaded_trade_count = loaded.trades().len();
    let loaded_stop_count = loaded.pending_stop_count();

    println!("  Loaded MSFT from file");
    println!(
        "  Determinism check (save/load):"
    );
    println!(
        "    BBO:    original={:?} loaded={:?} match={}",
        orig_bbo, loaded_bbo, orig_bbo == loaded_bbo
    );
    println!(
        "    Trades: original={} loaded={} match={}",
        orig_trade_count, loaded_trade_count, orig_trade_count == loaded_trade_count
    );
    println!(
        "    Stops:  original={} loaded={} match={}",
        orig_stop_count, loaded_stop_count, orig_stop_count == loaded_stop_count
    );

    // Also demo in-memory replay
    let events_vec = msft_ex.events().to_vec();
    let replayed = Exchange::replay(&events_vec);
    let replay_bbo = replayed.best_bid_ask();
    println!(
        "\n  In-memory replay: BBO={:?} match={}",
        replay_bbo,
        replay_bbo == orig_bbo
    );

    // Clean up temp file
    let _ = std::fs::remove_file(save_path);
    println!("  Cleaned up {}", save_path.display());

    // =====================================================================
    // Closing: Capability Checklist
    // =====================================================================
    print_section("Capability Checklist");

    let capabilities = [
        ("Limit orders (GTC)",           "Phase 1"),
        ("Limit orders (IOC)",           "Phase 2"),
        ("Limit orders (FOK)",           "Phase 3"),
        ("Market orders",                "Phase 2"),
        ("Multi-level sweeps",           "Phase 2"),
        ("Partial fills",                "Phase 2"),
        ("FOK rejection",                "Phase 3"),
        ("Order cancellation (O(1))",    "Phase 5"),
        ("Order modification",           "Phase 5"),
        ("Stop-market",                  "Phase 5,7"),
        ("Stop-limit",                   "Phase 5"),
        ("Trailing stop (Fixed)",        "Phase 5,6"),
        ("Trailing stop (Percentage)",   "Phase 5"),
        ("Stop cascade",                 "Phase 7"),
        ("L1 (BBO) snapshots",           "Phase 1,7"),
        ("L2 (depth) snapshots",         "Phase 1,6"),
        ("L3 (full book) snapshots",     "Phase 6"),
        ("Spread",                       "Phase 6"),
        ("Order book imbalance",         "Phase 6"),
        ("Weighted midpoint",            "Phase 6"),
        ("Mid price",                    "Phase 6"),
        ("VWAP",                         "Phase 2"),
        ("Trade history",                "Phase 2,7"),
        ("MultiExchange",               "Phase 1"),
        ("best_prices()",               "Phase 1"),
        ("Portfolio + CostModel",        "Phase 4"),
        ("LOBFill rebalancing",          "Phase 4"),
        ("SimpleFill (via backtest)",    "Phase 8"),
        ("Position tracking (VWAP)",     "Phase 4"),
        ("Returns & equity curve",       "Phase 8"),
        ("Strategy trait",               "Phase 8"),
        ("run_backtest()",              "Phase 8"),
        ("Metrics (Sharpe etc.)",        "Phase 8"),
        ("Event recording",              "Phase 10"),
        ("Deterministic replay",         "Phase 10"),
        ("Persistence (save/load)",      "Phase 10"),
        ("compact()",                   "Phase 9"),
        ("clear_trades()",             "Phase 9"),
        ("clear_order_history()",      "Phase 9"),
    ];

    for (cap, phase) in &capabilities {
        println!("  [x] {cap:<32} {phase}");
    }

    // Summary stats
    let total_trades: usize = [&aapl, &msft, &goog]
        .iter()
        .map(|sym| multi.get(sym).map(|ex| ex.trades().len()).unwrap_or(0))
        .sum();
    let total_events: usize = [&aapl, &msft, &goog]
        .iter()
        .map(|sym| multi.get(sym).map(|ex| ex.events().len()).unwrap_or(0))
        .sum();

    println!("\n  Summary:");
    println!("    Capabilities exercised: {}", capabilities.len());
    println!("    Total trades (remaining after clear): {total_trades}");
    println!("    Total events recorded: {total_events}");
    println!("\n  Trading day complete.\n");
}

// === Helper Functions ===

#[cfg(all(feature = "portfolio", feature = "persistence"))]
fn print_section(title: &str) {
    println!("\n{}", "=".repeat(64));
    println!("  {title}");
    println!("{}\n", "=".repeat(64));
}

#[cfg(all(feature = "portfolio", feature = "persistence"))]
fn print_book_snap(indent: &str, snap: &nanobook::BookSnapshot) {
    println!("{indent}  Asks:");
    for level in snap.asks.iter().rev() {
        println!(
            "{indent}    {} × {} ({} orders)",
            level.price, level.quantity, level.order_count
        );
    }
    println!("{indent}  ———————————————————");
    println!("{indent}  Bids:");
    for level in &snap.bids {
        println!(
            "{indent}    {} × {} ({} orders)",
            level.price, level.quantity, level.order_count
        );
    }
}

#[cfg(all(feature = "portfolio", feature = "persistence"))]
fn cents(v: i64) -> String {
    let dollars = v / 100;
    let c = (v % 100).abs();
    if v < 0 {
        format!("-${}.{:02}", dollars.abs(), c)
    } else {
        format!("${dollars}.{c:02}")
    }
}

#[cfg(not(all(feature = "portfolio", feature = "persistence")))]
fn main() {
    eprintln!("This example requires the 'portfolio' and 'persistence' features.");
    eprintln!("Run with: cargo run --features \"portfolio,persistence\" --example trading_day");
}
