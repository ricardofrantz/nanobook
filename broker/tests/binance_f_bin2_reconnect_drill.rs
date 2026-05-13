#![cfg(feature = "binance")]

//! F-bin2 integration tests for the Binance reconnect drill.

mod mock_binance;

use std::collections::HashSet;
use std::time::Instant;

use mock_binance::MockBinance;
use nanobook::Symbol;
use nanobook_broker::binance::{BinanceBroker, ConnectionMode, Discrepancy, DiscrepancyReport};
use nanobook_broker::types::{
    BrokerOrder, BrokerOrderType, BrokerSide, ClientOrderId, OrderId, OrderState,
};

fn btc_buy_order(client_order_id: &str) -> BrokerOrder {
    BrokerOrder {
        symbol: Symbol::new("BTC"),
        side: BrokerSide::Buy,
        quantity: 1000,
        order_type: BrokerOrderType::Market,
        client_order_id: Some(ClientOrderId::new(client_order_id).unwrap()),
    }
}

fn reconcile_mock_binance(
    broker: &mut BinanceBroker,
    exchange: &MockBinance,
    tracked_order_ids: &[OrderId],
) -> DiscrepancyReport {
    let tracked: HashSet<u64> = tracked_order_ids.iter().map(|id| id.0).collect();
    let mut discrepancies = Vec::new();

    for exchange_order in exchange.get_open_orders() {
        let Some(order_id) = exchange_order
            .client_order_id
            .as_deref()
            .and_then(|_| exchange_order_id(exchange, &exchange_order))
        else {
            continue;
        };

        if !tracked.contains(&order_id.0) {
            discrepancies.push(Discrepancy::OrphanOrder { order_id });
            continue;
        }

        if let Some(cached) = broker.get_cached_order(order_id) {
            if cached.status != exchange_order.status {
                discrepancies.push(Discrepancy::OrderStatusMismatch {
                    order_id,
                    local_status: format!("{:?}", cached.status),
                    broker_status: exchange_order.status,
                });
            }
        }
    }

    let has_critical_issues = !discrepancies.is_empty();
    if has_critical_issues {
        broker.block_reconciliation();
    }

    DiscrepancyReport {
        discrepancies,
        has_critical_issues,
    }
}

fn exchange_order_id(exchange: &MockBinance, target: &mock_binance::MockOrder) -> Option<OrderId> {
    for id in 1..exchange.next_order_id() {
        let order = exchange.get_order(&id.to_string())?;
        if order.client_order_id == target.client_order_id
            && order.symbol == target.symbol
            && order.quantity == target.quantity
            && order.side == target.side
        {
            return Some(OrderId(id));
        }
    }
    None
}

#[test]
fn test_no_double_submit_on_reconnect() {
    let mut broker = BinanceBroker::new("test_key", "test_secret", true)
        .with_connection_mode(ConnectionMode::WebSocket);
    let exchange = MockBinance::new();
    let order = btc_buy_order("f-bin2-no-double-submit-1");

    assert_eq!(broker.connection_mode(), ConnectionMode::WebSocket);

    let exchange_order_id = exchange
        .submit_order(
            "BTC",
            "BUY",
            "1000",
            Some(order.client_order_id.as_ref().unwrap().as_str()),
        )
        .unwrap();
    let order_id = OrderId(exchange_order_id.parse().unwrap());
    broker.cache_order(
        order_id,
        order.symbol,
        order.quantity as i64,
        order.side,
        Some(order.client_order_id.as_ref().unwrap().as_str().to_string()),
    );

    exchange.simulate_partial_fill(&exchange_order_id).unwrap();
    broker.update_cached_order_status(order_id, OrderState::PartiallyFilled);

    exchange.simulate_websocket_disconnect();
    assert!(exchange.is_websocket_disconnected());

    exchange.simulate_websocket_reconnect();
    assert!(!exchange.is_websocket_disconnected());

    let report = reconcile_mock_binance(&mut broker, &exchange, &[order_id]);
    assert!(!report.has_critical_issues, "{report:?}");

    let duplicate = broker.submit_order_with_sequence(&order, None);
    assert!(
        matches!(
            duplicate,
            Err(nanobook_broker::BrokerError::DuplicateOrder { .. })
        ),
        "resume submission should be rejected by client_order_id cache, got {duplicate:?}"
    );
    assert_eq!(exchange.all_orders().len(), 1);
}

#[test]
fn test_reconnect_within_30s() {
    let mut broker = BinanceBroker::new("test_key", "test_secret", true)
        .with_connection_mode(ConnectionMode::WebSocket);
    let exchange = MockBinance::new();
    let mut tracked_order_ids = Vec::new();

    for sequence in 1..=5 {
        let client_order_id = format!("f-bin2-reconnect-{sequence}");
        let exchange_order_id = exchange
            .submit_order("BTC", "BUY", "1000", Some(&client_order_id))
            .unwrap();
        exchange.simulate_partial_fill(&exchange_order_id).unwrap();

        let order_id = OrderId(exchange_order_id.parse().unwrap());
        broker.cache_order(
            order_id,
            Symbol::new("BTC"),
            1000,
            BrokerSide::Buy,
            Some(client_order_id),
        );
        broker.update_cached_order_status(order_id, OrderState::PartiallyFilled);
        tracked_order_ids.push(order_id);
    }

    exchange.simulate_websocket_disconnect();
    assert!(exchange.is_websocket_disconnected());

    let start = Instant::now();
    exchange.simulate_websocket_reconnect();
    let report = reconcile_mock_binance(&mut broker, &exchange, &tracked_order_ids);
    let reconcile_duration_ms = start.elapsed().as_millis() as u64;

    println!("F-bin2 Reconnect Drill Timing Metrics:");
    println!("  Reconnect + reconcile duration: {reconcile_duration_ms}ms");
    println!("  Target: < 30000ms");

    assert!(
        reconcile_duration_ms < 30_000,
        "reconnect + reconcile took {reconcile_duration_ms}ms"
    );
    assert!(!report.has_critical_issues, "{report:?}");
    assert!(!exchange.is_websocket_disconnected());
}

#[test]
fn test_state_persists_across_disconnect() {
    let exchange = MockBinance::new();
    let order_id = exchange
        .submit_order("BTC", "BUY", "1000", Some("f-bin2-persist-1"))
        .unwrap();
    exchange.simulate_partial_fill(&order_id).unwrap();

    let before = exchange.get_order(&order_id).unwrap();
    assert_eq!(before.status, OrderState::PartiallyFilled);

    exchange.simulate_websocket_disconnect();
    exchange.simulate_websocket_reconnect();

    let after = exchange.get_order(&order_id).unwrap();
    assert_eq!(after.symbol, before.symbol);
    assert_eq!(after.quantity, before.quantity);
    assert_eq!(after.side, before.side);
    assert_eq!(after.status, before.status);
    assert_eq!(after.client_order_id, before.client_order_id);
    assert_eq!(exchange.all_orders().len(), 1);
}

#[test]
fn test_reconciliation_detects_orphan_order() {
    let mut broker = BinanceBroker::new("test_key", "test_secret", true)
        .with_connection_mode(ConnectionMode::WebSocket);
    let exchange = MockBinance::new();

    let tracked_exchange_id = exchange
        .submit_order("BTC", "BUY", "1000", Some("f-bin2-tracked-1"))
        .unwrap();
    let tracked_order_id = OrderId(tracked_exchange_id.parse().unwrap());
    broker.cache_order(
        tracked_order_id,
        Symbol::new("BTC"),
        1000,
        BrokerSide::Buy,
        Some("f-bin2-tracked-1".to_string()),
    );

    exchange.simulate_websocket_disconnect();
    let orphan_exchange_id = exchange
        .submit_order("ETH", "BUY", "500", Some("f-bin2-orphan-1"))
        .unwrap();
    let orphan_order_id = OrderId(orphan_exchange_id.parse().unwrap());
    exchange.simulate_websocket_reconnect();

    let report = reconcile_mock_binance(&mut broker, &exchange, &[tracked_order_id]);

    assert!(report.has_critical_issues);
    assert!(broker.is_reconciliation_blocked());
    assert!(
        report.discrepancies.iter().any(
            |d| matches!(d, Discrepancy::OrphanOrder { order_id } if *order_id == orphan_order_id)
        ),
        "expected orphan order {orphan_order_id:?} in {report:?}"
    );
}
