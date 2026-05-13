//! Tests for MockBinance and MockBroker.

mod mock_binance;

use mock_binance::{FailureMode, MockBinance, MockBroker};
use nanobook::Symbol;
use nanobook_broker::{
    Broker, BrokerError, BrokerOrder, BrokerOrderType, BrokerSide, ClientOrderId,
};
use std::time::SystemTime;

#[test]
fn test_mock_binance_submit_order() {
    let binance = MockBinance::new();

    let order_id = binance
        .submit_order("BTCUSDT", "BUY", "100", None)
        .unwrap();

    assert_eq!(order_id, "1");

    let order = binance.get_order(&order_id).unwrap();
    assert_eq!(order.symbol, "BTCUSDT");
    assert_eq!(order.side, "BUY");
    assert_eq!(order.quantity, "100");
    assert_eq!(order.client_order_id, None);
}

#[test]
fn test_mock_binance_client_order_id_dedup() {
    let binance = MockBinance::new();

    // Submit order with client_order_id
    let order_id1 = binance
        .submit_order("BTCUSDT", "BUY", "100", Some("test-cid-123"))
        .unwrap();
    assert_eq!(order_id1, "1");

    // Try to submit duplicate
    let result = binance.submit_order("BTCUSDT", "BUY", "100", Some("test-cid-123"));
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Duplicate client_order_id"));

    // Different client_order_id should work
    let order_id2 = binance
        .submit_order("BTCUSDT", "BUY", "100", Some("test-cid-456"))
        .unwrap();
    assert_eq!(order_id2, "2");
}

#[test]
fn test_mock_binance_get_order() {
    let binance = MockBinance::new();

    let order_id = binance
        .submit_order("ETHUSDT", "SELL", "50", Some("my-order-1"))
        .unwrap();

    let order = binance.get_order(&order_id).unwrap();
    assert_eq!(order.symbol, "ETHUSDT");
    assert_eq!(order.side, "SELL");
    assert_eq!(order.quantity, "50");
    assert_eq!(order.client_order_id, Some("my-order-1".to_string()));
    assert_eq!(order.status, nanobook_broker::OrderState::Submitted);
}

#[test]
fn test_mock_binance_cancel_order() {
    let binance = MockBinance::new();

    let order_id = binance
        .submit_order("BTCUSDT", "BUY", "100", None)
        .unwrap();

    // Verify initial status
    let order = binance.get_order(&order_id).unwrap();
    assert_eq!(order.status, nanobook_broker::OrderState::Submitted);

    // Cancel order
    binance.cancel_order(&order_id).unwrap();

    // Verify cancelled status
    let order = binance.get_order(&order_id).unwrap();
    assert_eq!(order.status, nanobook_broker::OrderState::Cancelled);
}

#[test]
fn test_mock_binance_get_open_orders() {
    let binance = MockBinance::new();

    // Submit multiple orders
    binance.submit_order("BTCUSDT", "BUY", "100", None).unwrap();
    binance.submit_order("ETHUSDT", "SELL", "50", None).unwrap();
    binance.submit_order("BTCUSDT", "BUY", "200", None).unwrap();

    // All should be open initially
    let open_orders = binance.get_open_orders();
    assert_eq!(open_orders.len(), 3);

    // Cancel one order
    binance.cancel_order("1").unwrap();

    // Now only 2 should be open
    let open_orders = binance.get_open_orders();
    assert_eq!(open_orders.len(), 2);
}

#[test]
fn test_mock_binance_reset() {
    let binance = MockBinance::new();

    // Submit orders
    binance.submit_order("BTCUSDT", "BUY", "100", Some("cid-1")).unwrap();
    binance.submit_order("ETHUSDT", "SELL", "50", None).unwrap();

    // Verify state
    assert_eq!(binance.all_orders().len(), 2);
    assert_eq!(binance.next_order_id(), 3);

    // Reset
    binance.reset();

    // Verify cleared state
    assert_eq!(binance.all_orders().len(), 0);
    assert_eq!(binance.next_order_id(), 1);

    // Duplicate client_order_id should now work
    binance.submit_order("BTCUSDT", "BUY", "100", Some("cid-1")).unwrap();
}

#[test]
fn test_mock_binance_failure_injection() {
    let binance = MockBinance::new();

    // Test NetworkTimeout
    binance.inject_failure(FailureMode::NetworkTimeout);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Network timeout"));
    binance.clear_failure();

    // Test RateLimitExceeded
    binance.inject_failure(FailureMode::RateLimitExceeded);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Rate limit exceeded"));
    binance.clear_failure();

    // Test InvalidSymbol
    binance.inject_failure(FailureMode::InvalidSymbol);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Invalid symbol"));
    binance.clear_failure();

    // Test InsufficientFunds
    binance.inject_failure(FailureMode::InsufficientFunds);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Insufficient funds"));
    binance.clear_failure();

    // Test DuplicateOrder
    binance.inject_failure(FailureMode::DuplicateOrder);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Duplicate order"));
    binance.clear_failure();

    // Test ServerError
    binance.inject_failure(FailureMode::ServerError);
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("Internal server error"));
    binance.clear_failure();

    // Verify normal operation after clearing failure
    let result = binance.submit_order("BTCUSDT", "BUY", "100", None);
    assert!(result.is_ok());
}

#[test]
fn test_mock_broker_positions() {
    let mut broker = MockBroker::new();

    // Set up mock positions
    let positions = vec![nanobook_broker::Position {
        symbol: Symbol::new("BTC"),
        quantity: 100,
        avg_cost_cents: 50000_00,
        market_value_cents: 50000_00 * 100,
        unrealized_pnl_cents: 0,
    }];
    broker = broker.with_positions(positions);

    broker.connect().unwrap();

    let retrieved = broker.positions().unwrap();
    assert_eq!(retrieved.len(), 1);
    assert_eq!(retrieved[0].symbol, Symbol::new("BTC"));
    assert_eq!(retrieved[0].quantity, 100);
}

#[test]
fn test_mock_broker_submit_order() {
    let mut broker = MockBroker::new();
    broker.connect().unwrap();

    let order = BrokerOrder {
        symbol: Symbol::new("BTCUSDT"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    let order_id = broker.submit_order(&order).unwrap();
    assert_eq!(order_id, nanobook_broker::OrderId(1));

    // Verify order was stored in underlying MockBinance
    let binance_order = broker.binance().get_order("1").unwrap();
    assert_eq!(binance_order.symbol, "BTCUSDT");
    assert_eq!(binance_order.side, "BUY");
}

#[test]
fn test_mock_broker_submit_order_with_client_order_id() {
    let mut broker = MockBroker::new();
    broker.connect().unwrap();

    let client_id = ClientOrderId::new("my-custom-id-123").unwrap();
    let order = BrokerOrder {
        symbol: Symbol::new("BTCUSDT"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(client_id),
    };

    let order_id = broker.submit_order(&order).unwrap();
    assert_eq!(order_id, nanobook_broker::OrderId(1));

    // Verify client_order_id was stored
    let binance_order = broker.binance().get_order("1").unwrap();
    assert_eq!(binance_order.client_order_id, Some("my-custom-id-123".to_string()));
}

#[test]
fn test_mock_broker_quote() {
    let mut broker = MockBroker::new();

    // Set up mock quote
    let symbol = Symbol::new("BTCUSDT");
    let quote = nanobook_broker::Quote {
        symbol,
        bid_cents: 50000_00,
        ask_cents: 50010_00,
        last_cents: 50005_00,
        volume: 1000,
        timestamp: SystemTime::now(),
    };
    broker = broker.with_quote(symbol, quote);

    broker.connect().unwrap();

    let retrieved = broker.quote(&symbol).unwrap();
    assert_eq!(retrieved.bid_cents, 50000_00);
    assert_eq!(retrieved.ask_cents, 50010_00);
}

#[test]
fn test_mock_broker_not_connected() {
    let broker = MockBroker::new();

    // Should fail when not connected
    assert!(matches!(broker.positions(), Err(BrokerError::NotConnected)));
    assert!(matches!(broker.account(), Err(BrokerError::NotConnected)));

    let order = BrokerOrder {
        symbol: Symbol::new("BTCUSDT"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };
    assert!(matches!(broker.submit_order(&order), Err(BrokerError::NotConnected)));
}

#[test]
fn test_mock_broker_order_status() {
    let mut broker = MockBroker::new();
    broker.connect().unwrap();

    let order = BrokerOrder {
        symbol: Symbol::new("BTCUSDT"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    let order_id = broker.submit_order(&order).unwrap();
    assert_eq!(order_id, nanobook_broker::OrderId(1));

    let status = broker.order_status(order_id).unwrap();
    assert_eq!(status.id, order_id);
    assert_eq!(status.status, nanobook_broker::OrderState::Submitted);
}

#[test]
fn test_mock_broker_cancel_order() {
    let mut broker = MockBroker::new();
    broker.connect().unwrap();

    let order = BrokerOrder {
        symbol: Symbol::new("BTCUSDT"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    let order_id = broker.submit_order(&order).unwrap();
    broker.cancel_order(order_id).unwrap();

    let status = broker.order_status(order_id).unwrap();
    assert_eq!(status.status, nanobook_broker::OrderState::Cancelled);
}

#[test]
fn test_mock_binance_order_ids_are_monotonic() {
    let binance = MockBinance::new();

    let id1 = binance.submit_order("BTCUSDT", "BUY", "100", None).unwrap();
    let id2 = binance.submit_order("BTCUSDT", "BUY", "100", None).unwrap();
    let id3 = binance.submit_order("BTCUSDT", "BUY", "100", None).unwrap();

    assert_eq!(id1, "1");
    assert_eq!(id2, "2");
    assert_eq!(id3, "3");
}
