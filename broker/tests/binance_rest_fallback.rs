//! Tests for Binance REST fallback polling mode (F-bin2 Phase 4).

#[cfg(feature = "binance")]
mod binance_rest_fallback_tests {
    use std::time::{Duration, SystemTime};

    use nanobook::Symbol;
    use nanobook_broker::binance::types::{AccountInfo, BalanceInfo, OrderInfo};
    use nanobook_broker::binance::{BinanceBroker, ConnectionMode};
    use nanobook_broker::types::{BrokerSide, OrderId, OrderState};

    fn account_info_fixture() -> AccountInfo {
        AccountInfo {
            balances: vec![
                BalanceInfo {
                    asset: "BTC".to_string(),
                    free: "1.00000000".to_string(),
                    locked: "0.50000000".to_string(),
                },
                BalanceInfo {
                    asset: "USDT".to_string(),
                    free: "100.00".to_string(),
                    locked: "0.00".to_string(),
                },
            ],
            positions: Vec::new(),
            open_orders: vec![OrderInfo {
                symbol: "BTCUSDT".to_string(),
                order_id: 42,
                status: "NEW".to_string(),
                side: "BUY".to_string(),
                orig_qty: "7".to_string(),
                executed_qty: "0".to_string(),
            }],
            can_trade: true,
        }
    }

    #[test]
    fn test_connection_mode_enum() {
        let broker = BinanceBroker::new("api-key", "secret-key", true)
            .with_connection_mode(ConnectionMode::Rest);

        assert_eq!(broker.connection_mode(), ConnectionMode::Rest);
        assert_ne!(ConnectionMode::WebSocket, ConnectionMode::Auto);
    }

    #[test]
    fn test_rest_polling_updates_state() {
        let broker = BinanceBroker::new("api-key", "secret-key", true)
            .with_connection_mode(ConnectionMode::Rest);

        broker
            .update_state_from_account_info(&account_info_fixture())
            .unwrap();

        let positions = broker.cached_positions();
        assert_eq!(positions.len(), 2);
        assert_eq!(positions[0].symbol, Symbol::try_new("BTC").unwrap());
        assert_eq!(positions[0].quantity, 150_000_000);

        let cached_order = broker.get_cached_order(OrderId(42)).unwrap();
        assert_eq!(cached_order.symbol, Symbol::try_new("BTC").unwrap());
        assert_eq!(cached_order.quantity, 7);
        assert_eq!(cached_order.side, BrokerSide::Buy);
        assert_eq!(cached_order.status, OrderState::Submitted);
        assert_eq!(cached_order.binance_order_id, "42");
    }

    #[test]
    fn test_mode_switching() {
        let mut broker = BinanceBroker::new("api-key", "secret-key", true);

        assert_eq!(broker.connection_mode(), ConnectionMode::Auto);

        broker.switch_to_rest_mode();
        assert_eq!(broker.connection_mode(), ConnectionMode::Rest);

        broker.switch_to_websocket_mode();
        assert_eq!(broker.connection_mode(), ConnectionMode::WebSocket);
    }

    #[test]
    fn test_polling_interval_enforcement() {
        let broker = BinanceBroker::new("api-key", "secret-key", true);

        assert!(broker.should_poll(None));

        let now = SystemTime::now();
        assert!(!broker.should_poll(Some(now)));

        let stale = now - Duration::from_millis(5001);
        assert!(broker.should_poll(Some(stale)));

        broker.set_last_account_poll(Some(now));
        broker.set_last_orders_poll(Some(stale));
        assert_eq!(broker.last_account_poll(), Some(now));
        assert_eq!(broker.last_orders_poll(), Some(stale));
    }

    #[test]
    fn test_mode_configuration() {
        let rest = BinanceBroker::new("api-key", "secret-key", true)
            .with_connection_mode(ConnectionMode::Rest);
        let websocket = BinanceBroker::new("api-key", "secret-key", true)
            .with_connection_mode(ConnectionMode::WebSocket);
        let auto = BinanceBroker::new("api-key", "secret-key", true);

        assert_eq!(rest.connection_mode(), ConnectionMode::Rest);
        assert_eq!(websocket.connection_mode(), ConnectionMode::WebSocket);
        assert_eq!(auto.connection_mode(), ConnectionMode::Auto);
    }
}
