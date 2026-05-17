//! Smoke test for MockTws - verifies basic functionality.

mod mock_tws;

use mock_tws::MockTws;

#[test]
fn test_mock_connects() {
    let mock = MockTws::new();
    assert!(!mock.is_connected());
    mock.connect().unwrap();
    assert!(mock.is_connected());
    assert!(mock.callbacks().contains(&"Connected".to_string()));
}

#[test]
fn test_mock_disconnects() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    mock.disconnect().unwrap();
    assert!(!mock.is_connected());
    assert!(mock.callbacks().contains(&"Disconnected".to_string()));
}

#[test]
fn test_submit_order_when_not_connected_fails() {
    let mock = MockTws::new();
    let result = mock.submit_order("AAPL", 100);
    assert!(result.is_err());
}

#[test]
fn test_submit_order_records_callback() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    assert!(order_id > 0);
    let callbacks = mock.callbacks();
    assert!(callbacks.iter().any(|c| c.starts_with("OrderSubmitted:")));
}

#[test]
fn test_order_ids_are_monotonic() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    let id1 = mock.submit_order("AAPL", 100).unwrap();
    let id2 = mock.submit_order("MSFT", 50).unwrap();
    let id3 = mock.submit_order("GOOGL", 25).unwrap();
    assert!(id1 < id2);
    assert!(id2 < id3);
}

#[test]
fn test_fill_order_updates_status() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 100).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Filled");
    assert_eq!(status.filled_quantity, 100);
}

#[test]
fn test_partial_fill() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.fill_order(order_id, 50).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "PartiallyFilled");
    assert_eq!(status.filled_quantity, 50);
}

#[test]
fn test_cancel_order() {
    let mock = MockTws::new();
    mock.connect().unwrap();
    let order_id = mock.submit_order("AAPL", 100).unwrap();
    mock.cancel_order(order_id).unwrap();
    let status = mock.order_status(order_id).unwrap();
    assert_eq!(status.status, "Cancelled");
}
