#![cfg(feature = "ibkr")]

use nanobook_broker::Broker;
use nanobook_broker::BrokerError;
use nanobook_broker::ibkr::IbkrBroker;
use nanobook_broker::ibkr::orders::{broker_order_status_from_ibkr_parts, map_ibkr_order_status};
use nanobook_broker::types::OrderState;

#[test]
fn test_open_orders_empty_when_not_connected() {
    let broker = IbkrBroker::new("127.0.0.1", 4002, 100);

    let result = broker.open_orders();

    assert!(matches!(result, Err(BrokerError::NotConnected)));
}

#[test]
fn test_ibkr_order_status_mapping() {
    assert_eq!(map_ibkr_order_status("Submitted"), OrderState::Submitted);
    assert_eq!(map_ibkr_order_status("PreSubmitted"), OrderState::Submitted);
    assert_eq!(
        map_ibkr_order_status("PendingSubmit"),
        OrderState::Submitted
    );
    assert_eq!(
        map_ibkr_order_status("PartiallyFilled"),
        OrderState::PartiallyFilled
    );
    assert_eq!(map_ibkr_order_status("Filled"), OrderState::Filled);
    assert_eq!(map_ibkr_order_status("Cancelled"), OrderState::Cancelled);
    assert_eq!(map_ibkr_order_status("ApiCancelled"), OrderState::Cancelled);
    assert_eq!(map_ibkr_order_status("Inactive"), OrderState::Rejected);
}

#[test]
fn test_ibkr_order_status_partially_filled() {
    let status = broker_order_status_from_ibkr_parts(42, "Submitted", 25.0, 75.0, 123.45).unwrap();

    assert_eq!(status.id.0, 42);
    assert_eq!(status.status, OrderState::PartiallyFilled);
    assert_eq!(status.filled_quantity, 25);
    assert_eq!(status.remaining_quantity, 75);
    assert_eq!(status.avg_fill_price_cents, 12_345);
}

#[test]
fn test_reconcile_state_requires_connection() {
    let broker = IbkrBroker::new("127.0.0.1", 4002, 100);

    let result = broker.reconcile_state();

    assert!(matches!(result, Err(BrokerError::NotConnected)));
}
