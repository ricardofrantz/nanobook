//! File-based persistence via JSON Lines event sourcing.
//!
//! Events are stored as one JSON object per line (`.jsonl` format).
//! This is simple, streamable, and human-readable.
//!
//! # Usage
//!
//! ```ignore
//! use nanobook::persistence;
//! use std::path::Path;
//!
//! // Save exchange state
//! exchange.save(Path::new("orders.jsonl")).unwrap();
//!
//! // Load from file
//! let exchange = Exchange::load(Path::new("orders.jsonl")).unwrap();
//! ```

use std::io::{self, BufRead, Write};
use std::path::Path;

use crate::Exchange;
use crate::event::Event;

/// Save events to a file in JSON Lines format.
///
/// Each event is serialized as one JSON object per line.
pub fn save_events(events: &[Event], path: &Path) -> io::Result<()> {
    let file = std::fs::File::create(path)?;
    let mut writer = io::BufWriter::new(file);

    for event in events {
        let json = serde_json::to_string(event).map_err(io::Error::other)?;
        writeln!(writer, "{}", json)?;
    }

    writer.flush()?;
    Ok(())
}

/// Load events from a JSON Lines file.
///
/// Each line is parsed as one JSON event object.
/// Empty lines are skipped.
pub fn load_events(path: &Path) -> io::Result<Vec<Event>> {
    let file = std::fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut events = Vec::new();

    for (line_num, line) in reader.lines().enumerate() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let event: Event = serde_json::from_str(line).map_err(|e| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("line {}: {}", line_num + 1, e),
            )
        })?;
        events.push(event);
    }

    Ok(events)
}

impl Exchange {
    /// Save the exchange's event log to a file.
    ///
    /// The file uses JSON Lines format (one event per line).
    /// Requires the `persistence` and `event-log` features.
    pub fn save(&self, path: &Path) -> io::Result<()> {
        save_events(self.events(), path)
    }

    /// Load an exchange from a saved event log file.
    ///
    /// Creates a new exchange and replays all events from the file.
    /// Requires the `persistence` and `event-log` features.
    pub fn load(path: &Path) -> io::Result<Self> {
        let events = load_events(path)?;
        Ok(Self::replay(&events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Price, Side, TimeInForce};
    use std::path::PathBuf;

    fn test_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("test_{}.jsonl", name))
    }

    #[test]
    fn save_and_load_round_trip() {
        let path = test_path("round_trip");

        let mut exchange = Exchange::new();
        exchange.submit_limit(Side::Sell, Price(101_00), 100, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(100_00), 200, TimeInForce::GTC);
        exchange.submit_limit(Side::Buy, Price(101_00), 50, TimeInForce::GTC);

        // Save
        exchange.save(&path).unwrap();

        // Load
        let loaded = Exchange::load(&path).unwrap();

        // Verify identical state
        assert_eq!(exchange.best_bid_ask(), loaded.best_bid_ask());
        assert_eq!(exchange.trades().len(), loaded.trades().len());

        for (orig, repl) in exchange.trades().iter().zip(loaded.trades().iter()) {
            assert_eq!(orig.price, repl.price);
            assert_eq!(orig.quantity, repl.quantity);
        }

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn save_and_load_events_directly() {
        let path = test_path("direct_events");

        let events = vec![
            Event::submit_limit(Side::Sell, Price(100_00), 100, TimeInForce::GTC),
            Event::submit_market(Side::Buy, 50),
            Event::cancel(crate::OrderId(1)),
        ];

        save_events(&events, &path).unwrap();
        let loaded = load_events(&path).unwrap();

        assert_eq!(events.len(), loaded.len());
        assert_eq!(events, loaded);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_nonexistent_file() {
        let result = Exchange::load(Path::new("nonexistent_file.jsonl"));
        assert!(result.is_err());
    }

    #[test]
    fn save_empty_exchange() {
        let path = test_path("empty");

        let exchange = Exchange::new();
        exchange.save(&path).unwrap();

        let loaded = Exchange::load(&path).unwrap();
        assert_eq!(loaded.best_bid(), None);
        assert_eq!(loaded.best_ask(), None);

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn round_trip_with_stop_orders() {
        let path = test_path("stops");

        let mut exchange = Exchange::new();
        exchange.submit_limit(Side::Sell, Price(100_00), 50, TimeInForce::GTC);
        exchange.submit_stop_market(Side::Buy, Price(100_00), 100);
        exchange.submit_limit(Side::Buy, Price(99_00), 200, TimeInForce::GTC);

        exchange.save(&path).unwrap();
        let loaded = Exchange::load(&path).unwrap();

        assert_eq!(exchange.best_bid_ask(), loaded.best_bid_ask());
        assert_eq!(exchange.pending_stop_count(), loaded.pending_stop_count());

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }
}
