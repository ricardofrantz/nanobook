use nanobook::itch::{ItchMessage, ItchParser};
use nanobook::{Exchange, OrderId, Price, PriceLevels, Side, TimeInForce};
use serde_json::json;
use std::collections::{HashMap, HashSet};
use std::fs::{File, create_dir_all};
use std::io::{self, BufReader, BufWriter, Error, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Config {
    input: PathBuf,
    output_dir: PathBuf,
    max_messages: Option<u64>,
    /// Skip the first N events from latency percentile pools. They pay
    /// allocator warmup, page-fault, and HashMap-rehash costs that
    /// pollute p99 tails. Warmup events still get written to the
    /// event log (so book reconstruction is unaffected); only their
    /// latency fields are emitted as JSON `null`.
    warmup_events: u64,
    /// Emit the top-5 book snapshot every N events. `1` = every event
    /// (preserves prior behavior; needed if report.py samples random
    /// timestamps). Higher = much faster wall-clock throughput at the
    /// cost of fewer snapshots. `0` = never emit snapshots.
    snapshot_every: u64,
}

#[derive(Debug, Clone)]
struct RestingOrder {
    symbol: String,
    order_id: OrderId,
    side: Side,
    price: Price,
    remaining: u64,
}

#[derive(Debug, Default)]
struct ReplayStats {
    messages: u64,
    add_orders: u64,
    executions: u64,
    cancels: u64,
    deletes: u64,
    replaces: u64,
    trades: u64,
    trade_events: u64,
    ignored: u64,
    unknown_reductions: u64,
    invariant_violations: u64,
    emitted_events: u64,
    total_volume: u64,
    first_timestamp: Option<u64>,
    last_timestamp: Option<u64>,
    unique_symbols: HashSet<String>,
}

fn main() -> io::Result<()> {
    let config = parse_args(std::env::args().skip(1))?;
    create_dir_all(&config.output_dir)?;

    let input = File::open(&config.input)?;
    let mut parser = ItchParser::new(BufReader::new(input));
    let mut replay = Replay::new(
        &config.output_dir,
        config.warmup_events,
        config.snapshot_every,
    )?;

    loop {
        let parse_started = Instant::now();
        let Some(message) = parser.next_message()? else {
            break;
        };
        let parse_latency_ns = parse_started.elapsed().as_nanos();
        replay.apply(message, parse_latency_ns)?;
        if config
            .max_messages
            .is_some_and(|max| replay.stats.messages >= max)
        {
            break;
        }
    }

    replay.write_summary()
}

fn parse_args(args: impl IntoIterator<Item = String>) -> io::Result<Config> {
    let mut input = None;
    let mut output_dir = PathBuf::from("examples/itch-replay/data/replay");
    let mut max_messages = None;
    // Default warmup: skip first 1000 events from latency percentiles.
    // Standard practice for benchmark hygiene on JIT-free Rust binaries
    // is enough to clear allocator and page-fault costs.
    let mut warmup_events: u64 = 1000;
    // Default `1` preserves prior behavior bit-for-bit so report.py
    // golden outputs do not change.
    let mut snapshot_every: u64 = 1;

    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--input" | "-i" => input = Some(PathBuf::from(next_value(&mut args, &arg)?)),
            "--output-dir" | "-o" => output_dir = PathBuf::from(next_value(&mut args, &arg)?),
            "--max-messages" => {
                max_messages = Some(parse_u64(&next_value(&mut args, &arg)?, &arg)?);
            }
            "--warmup" => warmup_events = parse_u64(&next_value(&mut args, &arg)?, &arg)?,
            "--snapshot-every" => {
                snapshot_every = parse_u64(&next_value(&mut args, &arg)?, &arg)?;
            }
            "--help" | "-h" => return Err(Error::new(ErrorKind::InvalidInput, usage())),
            other => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument {other}\n\n{}", usage()),
                ));
            }
        }
    }

    Ok(Config {
        input: input.ok_or_else(|| Error::new(ErrorKind::InvalidInput, usage()))?,
        output_dir,
        max_messages,
        warmup_events,
        snapshot_every,
    })
}

fn next_value(args: &mut impl Iterator<Item = String>, flag: &str) -> io::Result<String> {
    args.next()
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, format!("{flag} requires a value")))
}

fn parse_u64(value: &str, flag: &str) -> io::Result<u64> {
    value.parse::<u64>().map_err(|err| {
        Error::new(
            ErrorKind::InvalidInput,
            format!("{flag} must be an unsigned integer: {err}"),
        )
    })
}

fn usage() -> &'static str {
    "usage: cargo run --example itch-replay -- --input FILE [--output-dir DIR] \
     [--max-messages N] [--warmup N] [--snapshot-every N]\n\n\
     --warmup N           Skip first N events from latency percentile pool (default 1000)\n\
     --snapshot-every N   Emit top-5 book snapshot every N events (default 1 = every event, 0 = never)"
}

struct Replay {
    exchanges: HashMap<String, Exchange>,
    orders: HashMap<u64, RestingOrder>,
    last_timestamp_by_symbol: HashMap<String, u64>,
    event_log: BufWriter<File>,
    invariant_log: BufWriter<File>,
    summary: BufWriter<File>,
    stats: ReplayStats,
    current_parse_latency_ns: u128,
    warmup_events: u64,
    snapshot_every: u64,
}

impl Replay {
    fn new(output_dir: &Path, warmup_events: u64, snapshot_every: u64) -> io::Result<Self> {
        Ok(Self {
            exchanges: HashMap::new(),
            orders: HashMap::new(),
            last_timestamp_by_symbol: HashMap::new(),
            event_log: BufWriter::new(File::create(output_dir.join("event-log.jsonl"))?),
            invariant_log: BufWriter::new(File::create(output_dir.join("invariants.log"))?),
            summary: BufWriter::new(File::create(output_dir.join("summary.txt"))?),
            stats: ReplayStats::default(),
            current_parse_latency_ns: 0,
            warmup_events,
            snapshot_every,
        })
    }

    /// True while we're inside the warmup window. Latency numbers from
    /// these events are emitted as JSON `null` so downstream
    /// percentile aggregators skip them.
    ///
    /// The window counts **emitted** (measured) events, not total ITCH
    /// messages. NASDAQ ITCH files begin with several thousand system
    /// events (StockDirectory, MarketHours, …) that are ignored by the
    /// replay and contribute zero latency samples; using
    /// `stats.messages` here would expire the warmup window before any
    /// tradeable Add Order has been measured.
    #[inline]
    fn is_warmup(&self) -> bool {
        self.stats.emitted_events <= self.warmup_events
    }

    /// True when this event should carry a top-5 book snapshot.
    #[inline]
    fn should_snapshot(&self) -> bool {
        self.snapshot_every > 0 && self.stats.messages % self.snapshot_every == 0
    }

    /// Either the recorded book snapshot or all-nulls — depending on
    /// `--snapshot-every`. Skipping the snapshot also skips the
    /// `iter_best_to_worst().take(5)` + JSON `Value` allocations
    /// that dominate wall-clock throughput.
    fn snapshot_or_null(
        &self,
        stock: &str,
    ) -> (Option<i64>, Option<i64>, Option<i64>, serde_json::Value) {
        if self.should_snapshot() {
            self.book_state(stock)
        } else {
            (None, None, None, serde_json::Value::Null)
        }
    }

    #[inline]
    fn parse_latency_json(&self) -> serde_json::Value {
        if self.is_warmup() {
            serde_json::Value::Null
        } else {
            json!(self.current_parse_latency_ns)
        }
    }

    #[inline]
    fn book_latency_json(&self, ns: u128) -> serde_json::Value {
        if self.is_warmup() {
            serde_json::Value::Null
        } else {
            json!(ns)
        }
    }

    fn apply(&mut self, message: ItchMessage, parse_latency_ns: u128) -> io::Result<()> {
        self.current_parse_latency_ns = parse_latency_ns;
        self.stats.messages += 1;

        match message {
            ItchMessage::AddOrder {
                timestamp,
                order_ref,
                side,
                shares,
                stock,
                price,
            } => self.add_order(timestamp, order_ref, side, shares, stock, price)?,
            ItchMessage::OrderExecuted {
                timestamp,
                order_ref,
                shares,
                match_number,
            } => self.reduce_order(timestamp, order_ref, shares, "execute", Some(match_number))?,
            ItchMessage::OrderExecutedWithPrice {
                timestamp,
                order_ref,
                shares,
                match_number,
                price,
                ..
            } => self.reduce_order_with_price(
                timestamp,
                order_ref,
                shares,
                "execute_with_price",
                Some(match_number),
                Some(price),
            )?,
            ItchMessage::OrderCancel {
                timestamp,
                order_ref,
                shares,
            } => self.reduce_order(timestamp, order_ref, shares, "cancel", None)?,
            ItchMessage::OrderDelete {
                timestamp,
                order_ref,
            } => self.delete_order(timestamp, order_ref)?,
            ItchMessage::OrderReplace {
                timestamp,
                old_order_ref,
                new_order_ref,
                shares,
                price,
            } => self.replace_order(timestamp, old_order_ref, new_order_ref, shares, price)?,
            ItchMessage::Trade {
                timestamp,
                side,
                shares,
                stock,
                price,
                match_number,
            } => {
                self.record_non_cross_trade(timestamp, side, shares, stock, price, match_number)?
            }
            ItchMessage::StockDirectory { stock, .. } => {
                self.exchanges.entry(stock).or_default();
                self.stats.ignored += 1;
            }
            ItchMessage::Other(_) => self.stats.ignored += 1,
        }

        Ok(())
    }

    fn add_order(
        &mut self,
        timestamp: u64,
        order_ref: u64,
        side: Side,
        shares: u32,
        stock: String,
        price: u32,
    ) -> io::Result<()> {
        self.stats.add_orders += 1;
        self.record_event_stats(timestamp, &stock, shares as u64);
        self.check_monotonic(&stock, timestamp)?;
        let nb_price = itch_price(price);
        let resting_before = self.total_resting(&stock);
        let update_started = Instant::now();
        let result =
            self.exchange_mut(&stock)
                .submit_limit(side, nb_price, shares as u64, TimeInForce::GTC);
        let book_update_latency_ns = update_started.elapsed().as_nanos();
        let resting_after = self.total_resting(&stock);
        if resting_after > resting_before + shares as u64 {
            self.write_violation(
                timestamp,
                &stock,
                format!(
                    "aggregate volume grew too much on add: before={resting_before}, after={resting_after}, qty={shares}"
                ),
            )?;
        }
        self.orders.insert(
            order_ref,
            RestingOrder {
                symbol: stock.clone(),
                order_id: result.order_id,
                side,
                price: nb_price,
                remaining: result.resting_quantity,
            },
        );
        self.stats.trades += result.trades.len() as u64;
        let (best_bid, best_ask, spread, book) = self.snapshot_or_null(&stock);
        self.write_event(json!({
            "timestamp": timestamp,
            "type": "add",
            "stock": stock,
            "itch_order_ref": order_ref,
            "order_id": result.order_id.0,
            "nanobook_order_id": result.order_id.0,
            "side": side_name(side),
            "price": nb_price.0,
            "qty": shares,
            "shares": shares,
            "resting": result.resting_quantity,
            "trades": result.trades.len(),
            "best_bid": best_bid,
            "best_ask": best_ask,
            "spread": spread,
            "book": book,
            "parse_latency_ns": self.parse_latency_json(),
            "book_update_latency_ns": self.book_latency_json(book_update_latency_ns),
            "strategy_to_order_latency_ns": 0,
        }))?;
        self.check_crossed_book(&stock, timestamp)
    }

    fn reduce_order(
        &mut self,
        timestamp: u64,
        order_ref: u64,
        shares: u32,
        action: &str,
        match_number: Option<u64>,
    ) -> io::Result<()> {
        self.reduce_order_with_price(timestamp, order_ref, shares, action, match_number, None)
    }

    fn reduce_order_with_price(
        &mut self,
        timestamp: u64,
        order_ref: u64,
        shares: u32,
        action: &str,
        match_number: Option<u64>,
        execution_price: Option<u32>,
    ) -> io::Result<()> {
        let Some(resting) = self.orders.get(&order_ref).cloned() else {
            self.stats.unknown_reductions += 1;
            return Ok(());
        };

        if action.starts_with("execute") {
            self.stats.executions += 1;
        } else {
            self.stats.cancels += 1;
        }
        self.record_event_stats(timestamp, &resting.symbol, shares as u64);
        self.check_monotonic(&resting.symbol, timestamp)?;
        let resting_before = self.total_resting(&resting.symbol);
        let reduction = shares as u64;
        let update_started = Instant::now();
        if reduction >= resting.remaining {
            self.exchange_mut(&resting.symbol).cancel(resting.order_id);
            self.orders.remove(&order_ref);
        } else {
            let new_remaining = resting.remaining - reduction;
            let result = self.exchange_mut(&resting.symbol).modify(
                resting.order_id,
                resting.price,
                new_remaining,
            );
            if let Some(new_order_id) = result.new_order_id {
                self.orders.insert(
                    order_ref,
                    RestingOrder {
                        order_id: new_order_id,
                        remaining: new_remaining,
                        ..resting.clone()
                    },
                );
            } else {
                self.stats.unknown_reductions += 1;
                self.orders.remove(&order_ref);
            }
            self.stats.trades += result.trades.len() as u64;
        }
        let book_update_latency_ns = update_started.elapsed().as_nanos();
        self.check_reduction_volume(&resting.symbol, timestamp, resting_before, reduction)?;

        let (best_bid, best_ask, spread, book) = self.snapshot_or_null(&resting.symbol);
        self.write_event(json!({
            "timestamp": timestamp,
            "type": action,
            "stock": resting.symbol,
            "itch_order_ref": order_ref,
            "order_id": resting.order_id.0,
            "nanobook_order_id": resting.order_id.0,
            "side": side_name(resting.side),
            "price": resting.price.0,
            "qty": shares,
            "shares": shares,
            "match_number": match_number,
            "execution_price": execution_price.map(|p| itch_price(p).0),
            "best_bid": best_bid,
            "best_ask": best_ask,
            "spread": spread,
            "book": book,
            "parse_latency_ns": self.parse_latency_json(),
            "book_update_latency_ns": self.book_latency_json(book_update_latency_ns),
            "strategy_to_order_latency_ns": 0,
        }))?;
        self.check_crossed_book(&resting.symbol, timestamp)
    }

    fn delete_order(&mut self, timestamp: u64, order_ref: u64) -> io::Result<()> {
        let Some(resting) = self.orders.remove(&order_ref) else {
            self.stats.unknown_reductions += 1;
            return Ok(());
        };
        self.stats.deletes += 1;
        self.record_event_stats(timestamp, &resting.symbol, resting.remaining);
        self.check_monotonic(&resting.symbol, timestamp)?;
        let resting_before = self.total_resting(&resting.symbol);
        let update_started = Instant::now();
        self.exchange_mut(&resting.symbol).cancel(resting.order_id);
        let book_update_latency_ns = update_started.elapsed().as_nanos();
        self.check_reduction_volume(
            &resting.symbol,
            timestamp,
            resting_before,
            resting.remaining,
        )?;
        let (best_bid, best_ask, spread, book) = self.snapshot_or_null(&resting.symbol);
        self.write_event(json!({
            "timestamp": timestamp,
            "type": "delete",
            "stock": resting.symbol,
            "itch_order_ref": order_ref,
            "order_id": resting.order_id.0,
            "nanobook_order_id": resting.order_id.0,
            "side": side_name(resting.side),
            "price": resting.price.0,
            "qty": resting.remaining,
            "shares": resting.remaining,
            "best_bid": best_bid,
            "best_ask": best_ask,
            "spread": spread,
            "book": book,
            "parse_latency_ns": self.parse_latency_json(),
            "book_update_latency_ns": self.book_latency_json(book_update_latency_ns),
            "strategy_to_order_latency_ns": 0,
        }))?;
        self.check_crossed_book(&resting.symbol, timestamp)
    }

    fn replace_order(
        &mut self,
        timestamp: u64,
        old_order_ref: u64,
        new_order_ref: u64,
        shares: u32,
        price: u32,
    ) -> io::Result<()> {
        let Some(old) = self.orders.remove(&old_order_ref) else {
            self.stats.unknown_reductions += 1;
            return Ok(());
        };
        self.stats.replaces += 1;
        self.record_event_stats(timestamp, &old.symbol, shares as u64);
        self.check_monotonic(&old.symbol, timestamp)?;
        let nb_price = itch_price(price);
        let resting_before = self.total_resting(&old.symbol);
        let update_started = Instant::now();
        let result = self
            .exchange_mut(&old.symbol)
            .modify(old.order_id, nb_price, shares as u64);
        let book_update_latency_ns = update_started.elapsed().as_nanos();
        let max_after = resting_before.saturating_sub(old.remaining) + shares as u64;
        if let Some(new_order_id) = result.new_order_id {
            self.orders.insert(
                new_order_ref,
                RestingOrder {
                    symbol: old.symbol.clone(),
                    order_id: new_order_id,
                    side: old.side,
                    price: nb_price,
                    remaining: shares as u64,
                },
            );
        } else {
            self.stats.unknown_reductions += 1;
        }
        let resting_after = self.total_resting(&old.symbol);
        if resting_after > max_after {
            self.write_violation(
                timestamp,
                &old.symbol,
                format!(
                    "aggregate volume grew too much on replace: before={resting_before}, after={resting_after}, old_qty={}, new_qty={shares}",
                    old.remaining
                ),
            )?;
        }
        self.stats.trades += result.trades.len() as u64;
        let (best_bid, best_ask, spread, book) = self.snapshot_or_null(&old.symbol);
        self.write_event(json!({
            "timestamp": timestamp,
            "type": "replace",
            "stock": old.symbol,
            "old_itch_order_ref": old_order_ref,
            "new_itch_order_ref": new_order_ref,
            "order_id": result.new_order_id.map(|id| id.0).unwrap_or(old.order_id.0),
            "old_nanobook_order_id": old.order_id.0,
            "new_nanobook_order_id": result.new_order_id.map(|id| id.0),
            "side": side_name(old.side),
            "price": nb_price.0,
            "qty": shares,
            "shares": shares,
            "best_bid": best_bid,
            "best_ask": best_ask,
            "spread": spread,
            "book": book,
            "parse_latency_ns": self.parse_latency_json(),
            "book_update_latency_ns": self.book_latency_json(book_update_latency_ns),
            "strategy_to_order_latency_ns": 0,
        }))?;
        self.check_crossed_book(&old.symbol, timestamp)
    }

    fn record_non_cross_trade(
        &mut self,
        timestamp: u64,
        side: Side,
        shares: u32,
        stock: String,
        price: u32,
        match_number: u64,
    ) -> io::Result<()> {
        self.stats.trades += 1;
        self.stats.trade_events += 1;
        self.record_event_stats(timestamp, &stock, shares as u64);
        self.check_monotonic(&stock, timestamp)?;
        self.write_event(json!({
            "timestamp": timestamp,
            "type": "non_cross_trade",
            "stock": stock,
            "side": side_name(side),
            "price": itch_price(price).0,
            "qty": shares,
            "shares": shares,
            "match_number": match_number,
            "parse_latency_ns": self.parse_latency_json(),
            "book_update_latency_ns": 0,
            "strategy_to_order_latency_ns": 0,
        }))
    }

    fn exchange_mut(&mut self, stock: &str) -> &mut Exchange {
        self.exchanges.entry(stock.to_string()).or_default()
    }

    fn check_monotonic(&mut self, stock: &str, timestamp: u64) -> io::Result<()> {
        if let Some(previous) = self
            .last_timestamp_by_symbol
            .insert(stock.to_string(), timestamp)
            && timestamp < previous
        {
            self.write_violation(
                timestamp,
                stock,
                format!("timestamp went backwards: previous={previous}, current={timestamp}"),
            )?;
        }
        Ok(())
    }

    fn check_crossed_book(&mut self, stock: &str, timestamp: u64) -> io::Result<()> {
        let crossed = self.exchanges.get(stock).and_then(|exchange| {
            let (bid, ask) = exchange.best_bid_ask();
            match (bid, ask) {
                (Some(bid), Some(ask)) if bid > ask => Some((bid.0, ask.0)),
                _ => None,
            }
        });

        if let Some((bid, ask)) = crossed {
            self.write_violation(
                timestamp,
                stock,
                format!("crossed book: best_bid={bid}, best_ask={ask}"),
            )?;
        }
        Ok(())
    }

    fn total_resting(&self, stock: &str) -> u64 {
        self.exchanges.get(stock).map_or(0, |exchange| {
            exchange.book().bids().total_quantity() + exchange.book().asks().total_quantity()
        })
    }

    fn book_state(
        &self,
        stock: &str,
    ) -> (Option<i64>, Option<i64>, Option<i64>, serde_json::Value) {
        let Some(exchange) = self.exchanges.get(stock) else {
            return (None, None, None, json!({ "bids": [], "asks": [] }));
        };
        let (bid, ask) = exchange.best_bid_ask();
        let bid = bid.map(|price| price.0);
        let ask = ask.map(|price| price.0);
        let spread = bid.zip(ask).map(|(bid, ask)| ask - bid);
        let book = json!({
            "bids": levels_json(exchange.book().bids()),
            "asks": levels_json(exchange.book().asks()),
        });
        (bid, ask, spread, book)
    }

    fn check_reduction_volume(
        &mut self,
        stock: &str,
        timestamp: u64,
        before: u64,
        reduction: u64,
    ) -> io::Result<()> {
        let after = self.total_resting(stock);
        if after > before || before - after > reduction {
            self.write_violation(
                timestamp,
                stock,
                format!(
                    "aggregate volume invalid reduction: before={before}, after={after}, reduction={reduction}"
                ),
            )?;
        }
        Ok(())
    }

    fn write_event(&mut self, event: serde_json::Value) -> io::Result<()> {
        serde_json::to_writer(&mut self.event_log, &event)?;
        self.event_log.write_all(b"\n")
    }

    fn record_event_stats(&mut self, timestamp: u64, stock: &str, qty: u64) {
        self.stats.emitted_events += 1;
        self.stats.total_volume += qty;
        self.stats.first_timestamp = Some(
            self.stats
                .first_timestamp
                .map_or(timestamp, |first| first.min(timestamp)),
        );
        self.stats.last_timestamp = Some(
            self.stats
                .last_timestamp
                .map_or(timestamp, |last| last.max(timestamp)),
        );
        self.stats.unique_symbols.insert(stock.to_string());
    }

    fn write_violation(&mut self, timestamp: u64, stock: &str, message: String) -> io::Result<()> {
        self.stats.invariant_violations += 1;
        writeln!(self.invariant_log, "{timestamp}\t{stock}\t{message}")
    }

    fn write_summary(mut self) -> io::Result<()> {
        writeln!(self.summary, "messages={}", self.stats.messages)?;
        writeln!(self.summary, "emitted_events={}", self.stats.emitted_events)?;
        writeln!(self.summary, "event_count.add={}", self.stats.add_orders)?;
        writeln!(
            self.summary,
            "event_count.execute={}",
            self.stats.executions
        )?;
        writeln!(self.summary, "event_count.cancel={}", self.stats.cancels)?;
        writeln!(self.summary, "event_count.delete={}", self.stats.deletes)?;
        writeln!(self.summary, "event_count.replace={}", self.stats.replaces)?;
        writeln!(
            self.summary,
            "event_count.trade={}",
            self.stats.trade_events
        )?;
        writeln!(self.summary, "add_orders={}", self.stats.add_orders)?;
        writeln!(self.summary, "executions={}", self.stats.executions)?;
        writeln!(self.summary, "cancels={}", self.stats.cancels)?;
        writeln!(self.summary, "deletes={}", self.stats.deletes)?;
        writeln!(self.summary, "replaces={}", self.stats.replaces)?;
        writeln!(self.summary, "trades={}", self.stats.trades)?;
        writeln!(self.summary, "ignored={}", self.stats.ignored)?;
        writeln!(
            self.summary,
            "unknown_reductions={}",
            self.stats.unknown_reductions
        )?;
        writeln!(
            self.summary,
            "invariant_violations={}",
            self.stats.invariant_violations
        )?;
        writeln!(self.summary, "open_orders={}", self.orders.len())?;
        writeln!(
            self.summary,
            "unique_symbols={}",
            self.stats.unique_symbols.len()
        )?;
        writeln!(self.summary, "total_volume={}", self.stats.total_volume)?;
        writeln!(
            self.summary,
            "first_timestamp={}",
            option_u64(self.stats.first_timestamp)
        )?;
        writeln!(
            self.summary,
            "last_timestamp={}",
            option_u64(self.stats.last_timestamp)
        )?;
        Ok(())
    }
}

fn itch_price(price: u32) -> Price {
    Price((price / 100) as i64)
}

fn side_name(side: Side) -> &'static str {
    match side {
        Side::Buy => "buy",
        Side::Sell => "sell",
    }
}

fn levels_json(levels: &PriceLevels) -> serde_json::Value {
    let levels: Vec<_> = levels
        .iter_best_to_worst()
        .take(5)
        .map(|(price, level)| {
            json!({
                "price": price.0,
                "shares": level.total_quantity(),
                "orders": level.order_count(),
            })
        })
        .collect();
    json!(levels)
}

fn option_u64(value: Option<u64>) -> String {
    value.map_or_else(|| "none".to_string(), |value| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_required_input() {
        let config = parse_args([
            "--input".to_string(),
            "slice.itch".to_string(),
            "--output-dir".to_string(),
            "out".to_string(),
            "--max-messages".to_string(),
            "10".to_string(),
        ])
        .unwrap();

        assert_eq!(
            config,
            Config {
                input: PathBuf::from("slice.itch"),
                output_dir: PathBuf::from("out"),
                max_messages: Some(10),
                warmup_events: 1000,
                snapshot_every: 1,
            }
        );
    }

    #[test]
    fn parses_warmup_and_snapshot_flags() {
        let config = parse_args([
            "--input".to_string(),
            "slice.itch".to_string(),
            "--warmup".to_string(),
            "5000".to_string(),
            "--snapshot-every".to_string(),
            "100".to_string(),
        ])
        .unwrap();

        assert_eq!(config.warmup_events, 5000);
        assert_eq!(config.snapshot_every, 100);
    }

    #[test]
    fn converts_itch_price_to_cents() {
        assert_eq!(itch_price(12_345_600), Price(123_456));
    }
}
