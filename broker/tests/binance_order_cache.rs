#![cfg(feature = "binance")]

use std::fs;
use std::path::PathBuf;

use chrono::Utc;
use nanobook::Symbol;
use nanobook_broker::binance::{BinanceBroker, BinanceOrderCache, CachedOrder};
use nanobook_broker::{BrokerSide, OrderId, OrderState};

fn test_cache_path(name: &str) -> PathBuf {
    PathBuf::from("target").join(format!(
        "binance_order_cache_{name}_{}.json",
        std::process::id()
    ))
}

#[test]
fn test_order_cache_new() {
    let cache = BinanceOrderCache::new();
    assert!(cache.orders.is_empty());
}

#[test]
fn test_cache_order_roundtrip() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);
    let order_id = OrderId(42);

    broker.cache_order(
        order_id,
        Symbol::new("BTC"),
        100,
        BrokerSide::Buy,
        Some("client-42".to_string()),
    );

    let cached = broker.get_cached_order(order_id).unwrap();
    assert_eq!(cached.symbol, Symbol::new("BTC"));
    assert_eq!(cached.quantity, 100);
    assert_eq!(cached.side, BrokerSide::Buy);
    assert_eq!(cached.status, OrderState::Submitted);
    assert_eq!(cached.binance_order_id, "42");
    assert_eq!(cached.client_order_id.as_deref(), Some("client-42"));
}

#[test]
fn test_cache_persistence() {
    let path = test_cache_path("persistence");
    let _ = fs::remove_file(&path);

    let mut cache = BinanceOrderCache::new();
    cache.orders.insert(
        OrderId(7),
        CachedOrder {
            symbol: Symbol::new("ETH"),
            quantity: 250,
            side: BrokerSide::Sell,
            status: OrderState::Submitted,
            binance_order_id: "7001".to_string(),
            client_order_id: Some("client-7".to_string()),
            submitted_at: Utc::now(),
        },
    );

    cache.save_to_disk(&path).unwrap();
    let loaded = BinanceOrderCache::load_from_disk(&path).unwrap();
    let cached = loaded.orders.get(&OrderId(7)).unwrap();

    assert_eq!(cached.symbol, Symbol::new("ETH"));
    assert_eq!(cached.quantity, 250);
    assert_eq!(cached.side, BrokerSide::Sell);
    assert_eq!(cached.status, OrderState::Submitted);
    assert_eq!(cached.binance_order_id, "7001");
    assert_eq!(cached.client_order_id.as_deref(), Some("client-7"));

    fs::remove_file(path).unwrap();
}

#[test]
fn test_cache_clear() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);
    let order_id = OrderId(10);
    broker.cache_order(order_id, Symbol::new("SOL"), 12, BrokerSide::Buy, None);

    assert!(broker.get_cached_order(order_id).is_some());
    broker.clear_cache();
    assert!(broker.get_cached_order(order_id).is_none());
}

#[test]
fn test_update_cached_order_status() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);
    let order_id = OrderId(11);
    broker.cache_order(order_id, Symbol::new("ADA"), 99, BrokerSide::Sell, None);

    broker.update_cached_order_status(order_id, OrderState::Filled);

    let cached = broker.get_cached_order(order_id).unwrap();
    assert_eq!(cached.status, OrderState::Filled);
}
