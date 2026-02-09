// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Tests for Binance API response parsing and auth — no live connection needed.

#[cfg(feature = "binance")]
mod binance_tests {
    use nanobook_broker::binance::auth;
    use nanobook_broker::binance::types::{AccountInfo, BookTicker, OrderResponse};

    // ========================================================================
    // HMAC-SHA256 signing
    // ========================================================================

    #[test]
    fn sign_binance_docs_example() {
        // Official Binance API documentation example
        let query = "symbol=LTCBTC&side=BUY&type=LIMIT&timeInForce=GTC\
                     &quantity=1&price=0.1&recvWindow=5000&timestamp=1499827319559";
        let secret = "NhqPtmdSJYdKjVHjA7PZj4Mge3R5YNiP1e3UZjInClVN65XAbvqqM6A7H5fATj0j";
        let sig = auth::sign(query, secret);
        assert_eq!(
            sig,
            "c8db56825ae71d6d79447849e617115f4a920fa2acdcab2b053c4b2838bd6b71"
        );
    }

    #[test]
    fn sign_empty_query() {
        let sig = auth::sign("", "secret");
        assert!(
            !sig.is_empty(),
            "empty query should still produce a signature"
        );
        assert_eq!(sig.len(), 64, "SHA256 hex is always 64 chars");
    }

    #[test]
    fn sign_deterministic() {
        let a = auth::sign("foo=bar", "key");
        let b = auth::sign("foo=bar", "key");
        assert_eq!(a, b, "same input must produce same signature");
    }

    #[test]
    fn sign_different_keys_differ() {
        let a = auth::sign("foo=bar", "key1");
        let b = auth::sign("foo=bar", "key2");
        assert_ne!(a, b, "different keys must produce different signatures");
    }

    // ========================================================================
    // AccountInfo parsing
    // ========================================================================

    #[test]
    fn parse_account_info_full() {
        let json = r#"{
            "balances": [
                { "asset": "BTC", "free": "1.00000000", "locked": "0.50000000" },
                { "asset": "USDT", "free": "10000.00", "locked": "0.00" }
            ],
            "canTrade": true
        }"#;

        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert_eq!(info.balances.len(), 2);
        assert!(info.can_trade);
        assert_eq!(info.balances[0].asset, "BTC");
        assert_eq!(info.balances[0].free, "1.00000000");
        assert_eq!(info.balances[0].locked, "0.50000000");
        assert_eq!(info.balances[1].asset, "USDT");
    }

    #[test]
    fn parse_account_info_empty_balances() {
        let json = r#"{ "balances": [] }"#;
        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert!(info.balances.is_empty());
        assert!(!info.can_trade); // defaults to false
    }

    #[test]
    fn parse_account_info_missing_can_trade() {
        let json = r#"{ "balances": [{ "asset": "ETH", "free": "5.0", "locked": "0.0" }] }"#;
        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert!(!info.can_trade); // serde default
        assert_eq!(info.balances.len(), 1);
    }

    #[test]
    fn parse_account_info_extra_fields_ignored() {
        let json = r#"{
            "makerCommission": 15,
            "takerCommission": 15,
            "balances": [],
            "canTrade": true,
            "permissions": ["SPOT"]
        }"#;

        let info: AccountInfo = serde_json::from_str(json).unwrap();
        assert!(info.can_trade);
    }

    // ========================================================================
    // OrderResponse parsing
    // ========================================================================

    #[test]
    fn parse_order_response_filled() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": 28,
            "status": "FILLED",
            "executedQty": "10.00000000",
            "cummulativeQuoteQty": "100000.00"
        }"#;

        let resp: OrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.symbol, "BTCUSDT");
        assert_eq!(resp.order_id, 28);
        assert_eq!(resp.status, "FILLED");
        assert_eq!(resp.executed_qty, "10.00000000");
        assert_eq!(resp.cummulative_quote_qty, "100000.00");
    }

    #[test]
    fn parse_order_response_new() {
        let json = r#"{
            "symbol": "ETHUSDT",
            "orderId": 123456,
            "status": "NEW",
            "executedQty": "0.00000000"
        }"#;

        let resp: OrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "NEW");
        assert_eq!(resp.executed_qty, "0.00000000");
        // cummulativeQuoteQty defaults to empty string
        assert_eq!(resp.cummulative_quote_qty, "");
    }

    #[test]
    fn parse_order_response_partial_fill() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": 42,
            "status": "PARTIALLY_FILLED",
            "executedQty": "3.50000000",
            "cummulativeQuoteQty": "35000.00"
        }"#;

        let resp: OrderResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.status, "PARTIALLY_FILLED");
    }

    // ========================================================================
    // BookTicker parsing
    // ========================================================================

    #[test]
    fn parse_book_ticker() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "bidPrice": "43250.50",
            "bidQty": "1.234",
            "askPrice": "43251.00",
            "askQty": "0.567"
        }"#;

        let ticker: BookTicker = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.symbol, "BTCUSDT");
        assert_eq!(ticker.bid_price, "43250.50");
        assert_eq!(ticker.ask_price, "43251.00");
        assert_eq!(ticker.bid_qty, "1.234");
        assert_eq!(ticker.ask_qty, "0.567");
    }

    #[test]
    fn parse_book_ticker_small_values() {
        let json = r#"{
            "symbol": "DOGEUSDT",
            "bidPrice": "0.08123",
            "bidQty": "100000.0",
            "askPrice": "0.08125",
            "askQty": "50000.0"
        }"#;

        let ticker: BookTicker = serde_json::from_str(json).unwrap();
        assert_eq!(ticker.bid_price, "0.08123");
    }

    // ========================================================================
    // Error cases — malformed JSON
    // ========================================================================

    #[test]
    fn reject_missing_required_fields() {
        // AccountInfo requires "balances"
        let json = r#"{ "canTrade": true }"#;
        assert!(serde_json::from_str::<AccountInfo>(json).is_err());
    }

    #[test]
    fn reject_wrong_type_order_id() {
        let json = r#"{
            "symbol": "BTCUSDT",
            "orderId": "not_a_number",
            "status": "NEW",
            "executedQty": "0.0"
        }"#;
        assert!(serde_json::from_str::<OrderResponse>(json).is_err());
    }

    #[test]
    fn reject_empty_json() {
        assert!(serde_json::from_str::<AccountInfo>("{}").is_err());
        assert!(serde_json::from_str::<OrderResponse>("{}").is_err());
        assert!(serde_json::from_str::<BookTicker>("{}").is_err());
    }

    // ========================================================================
    // BinanceBroker construction (no connection)
    // ========================================================================

    #[test]
    fn broker_not_connected_errors() {
        use nanobook_broker::Broker;
        use nanobook_broker::binance::BinanceBroker;

        let broker = BinanceBroker::new("test-key", "test-secret", true);
        // All operations should fail with NotConnected
        assert!(broker.positions().is_err());
        assert!(broker.account().is_err());
    }
}
