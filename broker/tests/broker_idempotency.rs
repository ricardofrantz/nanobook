use nanobook::Price;
use nanobook::Symbol;
use nanobook_broker::mock::MockBroker;
use nanobook_broker::{Broker, BrokerOrder, BrokerOrderType, BrokerSide, ClientOrderId, OrderId};

#[test]
fn broker_idempotency_deterministic_id_is_stable_across_calls() {
    let a = ClientOrderId::derive("sched-2026-04-20", "AAPL", BrokerSide::Buy, 100);
    let b = ClientOrderId::derive("sched-2026-04-20", "AAPL", BrokerSide::Buy, 100);

    assert_eq!(a, b);
}

#[test]
fn broker_idempotency_different_scopes_produce_different_ids() {
    let a = ClientOrderId::derive("sched-a", "AAPL", BrokerSide::Buy, 100);
    let b = ClientOrderId::derive("sched-b", "AAPL", BrokerSide::Buy, 100);

    assert_ne!(a, b);
}

#[test]
fn broker_idempotency_id_fits_binance_limit() {
    let a = ClientOrderId::derive("sched-2026-04-20", "AAPL", BrokerSide::Buy, 100);

    assert!(a.as_str().len() <= 36);
}

#[test]
fn broker_idempotency_id_is_hex_ascii_only() {
    let a = ClientOrderId::derive("sched", "AAPL", BrokerSide::Buy, 100);

    assert!(a.as_str().chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn broker_idempotency_mock_broker_records_client_order_id() {
    let mut broker = MockBroker::builder().build();
    broker.connect().unwrap();

    let client_order_id = ClientOrderId::derive("sched", "AAPL", BrokerSide::Buy, 100);
    let order = BrokerOrder {
        symbol: Symbol::new("AAPL"),
        side: BrokerSide::Buy,
        quantity: 100,
        order_type: BrokerOrderType::Limit(Price(18_500)),
        client_order_id: Some(client_order_id.clone()),
    };

    let id = broker.submit_order(&order).unwrap();
    assert_eq!(id, OrderId(1));

    let recorded = broker.submitted_orders();
    assert_eq!(recorded.len(), 1);
    assert_eq!(recorded[0].client_order_id, Some(client_order_id));
}
