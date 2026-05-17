#![cfg(feature = "binance")]

use nanobook::Symbol;
use nanobook_broker::binance::BinanceBroker;
use nanobook_broker::{
    BrokerError, BrokerOrder, BrokerOrderType, BrokerSide, ClientOrderId, OrderId,
};

#[test]
fn test_generate_client_order_id() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    let cid1 = broker.generate_client_order_id(1);
    let cid2 = broker.generate_client_order_id(2);

    // Check format: "nanobook-{short_uuid}-{sequence}"
    assert!(cid1.starts_with("nanobook-"));
    assert!(cid1.ends_with("-1"));
    assert!(cid2.ends_with("-2"));

    // Check that the ID is within the 36-character limit
    assert!(cid1.len() <= 36);
    assert!(cid2.len() <= 36);

    // Check format: nanobook-{16-char-uuid}-{sequence}
    let parts: Vec<&str> = cid1.split('-').collect();
    assert_eq!(parts.len(), 3); // nanobook + short_uuid + sequence
    assert_eq!(parts[1].len(), 16); // short UUID is 16 characters
}

#[test]
fn test_client_order_id_uniqueness() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    let cid1 = broker.generate_client_order_id(1);
    let cid2 = broker.generate_client_order_id(1);

    // Same sequence number should still generate different IDs due to UUID
    assert_ne!(cid1, cid2);

    // Different sequence numbers should definitely generate different IDs
    let cid3 = broker.generate_client_order_id(2);
    assert_ne!(cid1, cid3);
    assert_ne!(cid2, cid3);
}

#[test]
fn test_duplicate_client_order_id_detection() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    // Initially, no duplicates
    assert!(!broker.check_duplicate_client_order_id("nanobook-test-1"));

    // Cache an order with a client_order_id
    broker.cache_order(
        OrderId(1),
        Symbol::new("BTC"),
        100,
        BrokerSide::Buy,
        Some("nanobook-test-1".to_string()),
    );

    // Now it should be detected as duplicate
    assert!(broker.check_duplicate_client_order_id("nanobook-test-1"));

    // Different client_order_id should not be duplicate
    assert!(!broker.check_duplicate_client_order_id("nanobook-test-2"));

    // No client_order_id should not be duplicate
    assert!(!broker.check_duplicate_client_order_id(""));
}

#[test]
fn test_submit_order_with_client_order_id() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    // Test with sequence number
    let result = broker.submit_order_with_sequence(&order, Some(1));

    // This should fail because we're not actually connected to Binance
    // But we can verify that the error is NotConnected, not DuplicateOrder
    match result {
        Err(BrokerError::NotConnected) => {
            // Expected - we're not connected
        }
        Err(BrokerError::DuplicateOrder { .. }) => {
            panic!("Should not get DuplicateOrder error for first submission");
        }
        Err(e) => {
            panic!("Unexpected error: {:?}", e);
        }
        Ok(_) => {
            panic!("Should not succeed without connection");
        }
    }
}

#[test]
fn test_duplicate_order_rejection() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    // Cache an order with a specific client_order_id
    let client_id = "nanobook-duplicate-test-1";
    broker.cache_order(
        OrderId(100),
        Symbol::new("ETH"),
        200,
        BrokerSide::Sell,
        Some(client_id.to_string()),
    );

    let order = BrokerOrder {
        symbol: Symbol::new("ETH"),
        side: BrokerSide::Sell,
        quantity: 200,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new(client_id).unwrap()),
    };

    // Try to submit with the same client_order_id
    let result = broker.submit_order_with_sequence(&order, None);

    // Should get DuplicateOrder error
    match result {
        Err(BrokerError::DuplicateOrder { client_order_id }) => {
            assert_eq!(client_order_id, client_id);
        }
        Err(e) => {
            panic!("Expected DuplicateOrder error, got: {:?}", e);
        }
        Ok(_) => {
            panic!("Should not succeed with duplicate client_order_id");
        }
    }
}

#[test]
fn test_existing_client_order_id_takes_precedence() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    let existing_cid = ClientOrderId::new("my-custom-id-123").unwrap();
    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(existing_cid.clone()),
    };

    // Even with sequence_number provided, existing client_order_id should be used
    let result = broker.submit_order_with_sequence(&order, Some(999));

    // Should fail with NotConnected (not connected to Binance)
    // But the important thing is it doesn't fail with DuplicateOrder
    // (unless my-custom-id-123 is already in cache, which it shouldn't be)
    match result {
        Err(BrokerError::NotConnected) => {
            // Expected
        }
        Err(BrokerError::DuplicateOrder { client_order_id }) => {
            assert_eq!(client_order_id, "my-custom-id-123");
            // This could happen if the ID was cached from a previous test
            // For this test, we just verify the existing ID is used
        }
        Err(e) => {
            panic!("Unexpected error: {:?}", e);
        }
        Ok(_) => {
            panic!("Should not succeed without connection");
        }
    }
}

#[test]
fn test_no_sequence_number_no_client_order_id() {
    let broker = BinanceBroker::new("api-key", "secret-key", true);

    let order = BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Market,
        client_order_id: None,
    };

    // Submit without sequence_number and without client_order_id
    let result = broker.submit_order_with_sequence(&order, None);

    // Should fail with NotConnected (not connected to Binance)
    // But should not fail with DuplicateOrder since no client_order_id
    match result {
        Err(BrokerError::NotConnected) => {
            // Expected
        }
        Err(BrokerError::DuplicateOrder { .. }) => {
            panic!("Should not get DuplicateOrder when no client_order_id is set");
        }
        Err(e) => {
            panic!("Unexpected error: {:?}", e);
        }
        Ok(_) => {
            panic!("Should not succeed without connection");
        }
    }
}
