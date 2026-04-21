#![cfg(feature = "ibkr")]
// Allow cents-style literals in tests, e.g. 185_05 for $185.05.
#![allow(clippy::inconsistent_digit_grouping)]

use nanobook::Price;
use nanobook::Symbol;
use nanobook_broker::error::BrokerError;
use nanobook_broker::ibkr::orders::encode_order;
use nanobook_broker::types::{BestQuote, BrokerOrder, BrokerOrderType, BrokerSide};

fn mk_order(side: BrokerSide, order_type: BrokerOrderType) -> BrokerOrder {
    BrokerOrder {
        symbol: Symbol::new("AAPL"),
        side,
        quantity: 100,
        order_type,
        client_order_id: None,
    }
}

#[cfg(not(feature = "strict-market-reject"))]
#[test]
fn market_buy_with_quote_uses_ask_plus_50bps() {
    let quote = BestQuote {
        bid_cents: 184_95,
        ask_cents: 185_05,
    };
    let (price, qty) = encode_order(
        &mk_order(BrokerSide::Buy, BrokerOrderType::Market),
        Some(&quote),
    )
    .unwrap();

    assert!((price - 185.97525).abs() < 1e-6, "got {price}");
    assert_eq!(qty, 100.0);
}

#[cfg(not(feature = "strict-market-reject"))]
#[test]
fn market_sell_with_quote_uses_bid_minus_50bps() {
    let quote = BestQuote {
        bid_cents: 184_95,
        ask_cents: 185_05,
    };
    let (price, _) = encode_order(
        &mk_order(BrokerSide::Sell, BrokerOrderType::Market),
        Some(&quote),
    )
    .unwrap();

    assert!((price - 184.02525).abs() < 1e-6, "got {price}");
}

#[cfg(not(feature = "strict-market-reject"))]
#[test]
fn market_without_quote_returns_error() {
    let res = encode_order(&mk_order(BrokerSide::Buy, BrokerOrderType::Market), None);

    assert!(matches!(
        res,
        Err(BrokerError::NoQuoteForMarketOrder { .. })
    ));
}

#[cfg(not(feature = "strict-market-reject"))]
#[test]
fn encoded_price_is_never_the_legacy_hack() {
    let quote = BestQuote {
        bid_cents: 184_95,
        ask_cents: 185_05,
    };
    let (price, _) = encode_order(
        &mk_order(BrokerSide::Buy, BrokerOrderType::Market),
        Some(&quote),
    )
    .unwrap();

    assert!(
        price < 10_000.0,
        "legacy $999,999.99 hack must never re-appear"
    );
}

#[test]
fn limit_order_price_is_verbatim() {
    let (price, _) = encode_order(
        &mk_order(BrokerSide::Buy, BrokerOrderType::Limit(Price(185_00))),
        None,
    )
    .unwrap();

    assert!((price - 185.0).abs() < 1e-9);
}

#[cfg(feature = "strict-market-reject")]
#[test]
fn strict_feature_rejects_all_market_orders() {
    let quote = BestQuote {
        bid_cents: 184_95,
        ask_cents: 185_05,
    };
    let res = encode_order(
        &mk_order(BrokerSide::Buy, BrokerOrderType::Market),
        Some(&quote),
    );

    assert!(matches!(res, Err(BrokerError::MarketOrderRejected)));
}
