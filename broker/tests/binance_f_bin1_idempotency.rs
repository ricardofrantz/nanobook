#![cfg(feature = "binance")]

//! F-bin1 integration tests for Binance idempotency proof.
//!
//! Tests end-to-end idempotency behavior using MockBroker with audit log support.

use nanobook::Symbol;
use tempfile::TempDir;

use nanobook_broker::Broker;
use nanobook_broker::error::BrokerError;
use nanobook_broker::types::{BrokerOrder, BrokerOrderType, BrokerSide, ClientOrderId};

mod mock_binance;
use mock_binance::MockBroker;

#[test]
fn test_f_bin1_idempotency_duplicate_submission() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    let mut broker = MockBroker::new().with_audit_log_path(log_path.clone());
    broker.connect().unwrap();

    // Create an order with a client_order_id containing sequence number
    let client_order_id = ClientOrderId::new("nanobook-test-uuid-1").unwrap();
    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(client_order_id.clone()),
    };

    // First submission should succeed
    let result1 = broker.submit_order(&order);
    assert!(result1.is_ok(), "First submission should succeed");
    let order_id1 = result1.unwrap();

    // Attempt to submit the same order again
    let result2 = broker.submit_order(&order);
    assert!(matches!(result2, Err(BrokerError::DuplicateOrder { .. })), "Second submission should be rejected");

    // Verify only one order exists in MockBinance
    let all_orders = broker.binance().all_orders();
    assert_eq!(all_orders.len(), 1, "Should have exactly one order");

    // Verify audit log contains one OrderSubmitted and one IdempotencyRejection
    let content = std::fs::read_to_string(&log_path).unwrap();
    let order_submitted_count = content.lines().filter(|l| l.contains("order_submitted")).count();
    let rejection_count = content.lines().filter(|l| l.contains("idempotency_rejection")).count();
    assert_eq!(order_submitted_count, 1, "Should have one OrderSubmitted event");
    assert_eq!(rejection_count, 1, "Should have one IdempotencyRejection event");

    // Verify the order in MockBinance matches
    let stored_order = broker.binance().get_order(&order_id1.0.to_string());
    assert!(stored_order.is_some(), "Order should be stored in MockBinance");
}

#[test]
fn test_f_bin1_sequence_collision_different_orders() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    let mut broker = MockBroker::new().with_audit_log_path(log_path.clone());
    broker.connect().unwrap();

    // Submit order A with sequence 1
    let order_a = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new("nanobook-uuid-a-1").unwrap()),
    };

    let result_a = broker.submit_order(&order_a);
    assert!(result_a.is_ok(), "Order A submission should succeed");
    let order_id_a = result_a.unwrap();

    // Attempt to submit order B with the same sequence 1
    let order_b = BrokerOrder {
        symbol: Symbol::new("ETH"),
        side: BrokerSide::Sell,
        quantity: 500,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new("nanobook-uuid-b-1").unwrap()),
    };

    let result_b = broker.submit_order(&order_b);
    assert!(matches!(result_b, Err(BrokerError::DuplicateOrder { .. })), "Order B should be rejected due to sequence collision");

    // Verify only order A exists in MockBinance
    let all_orders = broker.binance().all_orders();
    assert_eq!(all_orders.len(), 1, "Should have exactly one order");

    // Verify the order is order A
    let stored_order = broker.binance().get_order(&order_id_a.0.to_string());
    assert!(stored_order.is_some(), "Order A should be stored");
    assert_eq!(stored_order.unwrap().symbol, "BTC", "Stored order should be BTC (order A)");

    // Verify audit log shows rejection for order B
    let content = std::fs::read_to_string(&log_path).unwrap();
    assert!(content.contains("idempotency_rejection"), "Audit log should contain rejection");
    assert!(content.contains("ETH"), "Rejection should mention ETH symbol");
}

#[test]
fn test_f_bin1_audit_log_verification() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    let mut broker = MockBroker::new().with_audit_log_path(log_path.clone());
    broker.connect().unwrap();

    // Submit an order with sequence 42
    let client_order_id = "nanobook-test-uuid-42";
    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new(client_order_id).unwrap()),
    };

    let result = broker.submit_order(&order);
    assert!(result.is_ok(), "Order submission should succeed");
    let order_id = result.unwrap();

    // Read audit log file
    let content = std::fs::read_to_string(&log_path).unwrap();

    // Verify audit log contains OrderSubmitted event with sequence 42
    assert!(content.contains("order_submitted"), "Audit log should contain order_submitted event");
    assert!(content.contains("42"), "Audit log should contain sequence number 42");

    // Verify audit log contains correct fields
    assert!(content.contains("BTC"), "Audit log should contain symbol BTC");
    assert!(content.contains(client_order_id), "Audit log should contain client_order_id");
    assert!(content.contains(&order_id.0.to_string()), "Audit log should contain order_id");

    // Verify audit log format is valid JSONL (one JSON object per line)
    for line in content.lines() {
        let _: serde_json::Value = serde_json::from_str(line)
            .expect("Each line should be valid JSON");
    }

    // Verify the JSON structure
    let first_line = content.lines().next().unwrap();
    let value: serde_json::Value = serde_json::from_str(first_line).unwrap();
    assert_eq!(value["event_type"], "order_submitted");
    assert_eq!(value["symbol"], "BTC");
    assert_eq!(value["sequence"], 42);
    assert_eq!(value["client_order_id"], client_order_id);
}

#[test]
fn test_f_bin1_cache_based_duplicate_detection() {
    // Create MockBroker WITHOUT audit log
    let mut broker = MockBroker::new();
    broker.connect().unwrap();

    // Submit order with client_order_id "test-id-123"
    let client_order_id = "test-id-123";
    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new(client_order_id).unwrap()),
    };

    let result1 = broker.submit_order(&order);
    assert!(result1.is_ok(), "First submission should succeed");

    // Attempt to submit same order with same client_order_id
    let result2 = broker.submit_order(&order);
    assert!(result2.is_err(), "Second submission should fail");

    // Verify it's rejected via cache check (MockBinance's duplicate check)
    match result2 {
        Err(BrokerError::Order(msg)) if msg.contains("Duplicate") => {
            // Expected - MockBinance returns error string for duplicates
        }
        _ => panic!("Expected duplicate order error from MockBinance cache"),
    }

    // Verify only one order exists
    let all_orders = broker.binance().all_orders();
    assert_eq!(all_orders.len(), 1, "Should have exactly one order");
}

#[test]
fn test_f_bin1_end_to_end_scenario() {
    let temp_dir = TempDir::new().unwrap();
    let log_path = temp_dir.path().join("audit.log");

    let mut broker = MockBroker::new().with_audit_log_path(log_path.clone());
    broker.connect().unwrap();

    // First run: submit multiple orders with sequences 1, 2, 3
    let orders = vec![
        BrokerOrder {
            symbol: Symbol::new("BTC"),
            side: BrokerSide::Buy,
            quantity: 1000,
            order_type: BrokerOrderType::Market,
            client_order_id: Some(ClientOrderId::new("nanobook-uuid1-1").unwrap()),
        },
        BrokerOrder {
            symbol: Symbol::new("ETH"),
            side: BrokerSide::Buy,
            quantity: 500,
            order_type: BrokerOrderType::Market,
            client_order_id: Some(ClientOrderId::new("nanobook-uuid2-2").unwrap()),
        },
        BrokerOrder {
            symbol: Symbol::new("SOL"),
            side: BrokerSide::Sell,
            quantity: 200,
            order_type: BrokerOrderType::Market,
            client_order_id: Some(ClientOrderId::new("nanobook-uuid3-3").unwrap()),
        },
    ];

    let mut first_run_order_ids = Vec::new();
    for order in &orders {
        let result = broker.submit_order(order);
        assert!(result.is_ok(), "First run submissions should succeed");
        first_run_order_ids.push(result.unwrap());
    }

    // Second run: attempt to submit same orders with same sequences
    for order in &orders {
        let result = broker.submit_order(order);
        assert!(matches!(result, Err(BrokerError::DuplicateOrder { .. })), "Second run submissions should be rejected");
    }

    // Verify only first-run orders exist in MockBinance
    let all_orders = broker.binance().all_orders();
    assert_eq!(all_orders.len(), 3, "Should have exactly three orders from first run");

    // Verify audit log shows correct sequence of events
    let content = std::fs::read_to_string(&log_path).unwrap();
    let order_submitted_count = content.lines().filter(|l| l.contains("order_submitted")).count();
    let rejection_count = content.lines().filter(|l| l.contains("idempotency_rejection")).count();
    assert_eq!(order_submitted_count, 3, "Should have three OrderSubmitted events");
    assert_eq!(rejection_count, 3, "Should have three IdempotencyRejection events");

    // Verify the order of events: first all submissions, then all rejections
    let lines: Vec<&str> = content.lines().collect();
    let mut found_rejection = false;
    for line in &lines {
        if line.contains("idempotency_rejection") {
            found_rejection = true;
        }
        if found_rejection && line.contains("order_submitted") {
            panic!("OrderSubmitted should not appear after IdempotencyRejection");
        }
    }
}
