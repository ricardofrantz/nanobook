// Allow our dollar.cents digit grouping convention (e.g., 100_00 = $100.00)
#![allow(clippy::inconsistent_digit_grouping)]

//! Tests for IBKR broker types and pure functions — no live TWS connection needed.

#[cfg(feature = "ibkr")]
mod ibkr_tests {
    use nanobook_broker::ibkr::orders::{OrderOutcome, rate_limit_delay};

    // ========================================================================
    // OrderOutcome
    // ========================================================================

    #[test]
    fn order_outcome_equality() {
        assert_eq!(OrderOutcome::Filled, OrderOutcome::Filled);
        assert_eq!(OrderOutcome::PartialFill, OrderOutcome::PartialFill);
        assert_eq!(OrderOutcome::Cancelled, OrderOutcome::Cancelled);
        assert_eq!(OrderOutcome::Failed, OrderOutcome::Failed);
    }

    #[test]
    fn order_outcome_inequality() {
        assert_ne!(OrderOutcome::Filled, OrderOutcome::Failed);
        assert_ne!(OrderOutcome::PartialFill, OrderOutcome::Cancelled);
    }

    #[test]
    fn order_outcome_debug() {
        let s = format!("{:?}", OrderOutcome::Filled);
        assert_eq!(s, "Filled");
    }

    // ========================================================================
    // rate_limit_delay
    // ========================================================================

    #[test]
    fn rate_limit_zero_returns_immediately() {
        let start = std::time::Instant::now();
        rate_limit_delay(0);
        assert!(start.elapsed().as_millis() < 50);
    }

    #[test]
    fn rate_limit_nonzero_sleeps() {
        let start = std::time::Instant::now();
        rate_limit_delay(100);
        assert!(start.elapsed().as_millis() >= 90);
    }

    // ========================================================================
    // IbkrBroker construction (no connection)
    // ========================================================================

    #[test]
    fn ibkr_broker_not_connected() {
        use nanobook_broker::Broker;
        use nanobook_broker::ibkr::IbkrBroker;

        let broker = IbkrBroker::new("127.0.0.1", 4002, 100);
        // Client should be None before connect
        assert!(broker.client().is_none());
        // All operations should fail with NotConnected
        assert!(broker.positions().is_err());
        assert!(broker.account().is_err());
    }
}

// ============================================================================
// Tests that don't require any feature flag — common types
// ============================================================================

use nanobook::Symbol;
use nanobook_broker::Broker;
use nanobook_broker::mock::{FillMode, MockBroker};
use nanobook_broker::types::*;

#[test]
fn broker_side_debug() {
    assert_eq!(format!("{:?}", BrokerSide::Buy), "Buy");
    assert_eq!(format!("{:?}", BrokerSide::Sell), "Sell");
}

#[test]
fn order_state_variants() {
    let states = [
        OrderState::Pending,
        OrderState::Submitted,
        OrderState::PartiallyFilled,
        OrderState::Filled,
        OrderState::Cancelled,
        OrderState::Rejected,
    ];
    // All states should be distinct
    for (i, a) in states.iter().enumerate() {
        for (j, b) in states.iter().enumerate() {
            if i == j {
                assert_eq!(a, b);
            } else {
                assert_ne!(a, b);
            }
        }
    }
}

#[test]
fn order_id_equality() {
    assert_eq!(OrderId(1), OrderId(1));
    assert_ne!(OrderId(1), OrderId(2));
}

#[test]
fn position_construction() {
    let pos = Position {
        symbol: Symbol::new("AAPL"),
        quantity: 100,
        avg_cost_cents: 185_00,
        market_value_cents: 100 * 185_00,
        unrealized_pnl_cents: 500_00,
    };
    assert_eq!(pos.symbol, Symbol::new("AAPL"));
    assert_eq!(pos.market_value_cents, 18500_00);
}

#[test]
fn account_fields() {
    let acct = Account {
        equity_cents: 1_000_000_00,
        buying_power_cents: 2_000_000_00,
        cash_cents: 500_000_00,
        gross_position_value_cents: 500_000_00,
    };
    assert_eq!(acct.equity_cents, 1_000_000_00);
}

#[test]
fn quote_construction() {
    let q = Quote {
        symbol: Symbol::new("MSFT"),
        bid_cents: 420_50,
        ask_cents: 421_00,
        last_cents: 420_75,
        volume: 1_000_000,
    };
    assert_eq!(q.bid_cents, 420_50);
    assert!(q.ask_cents > q.bid_cents);
}

// ============================================================================
// MockBroker — cross-feature tests (mock is always available)
// ============================================================================

fn aapl() -> Symbol {
    Symbol::new("AAPL")
}
fn msft() -> Symbol {
    Symbol::new("MSFT")
}

#[test]
fn mock_multiple_positions() {
    let mut broker = MockBroker::builder()
        .with_position(aapl(), 100, 185_00)
        .with_position(msft(), 50, 420_00)
        .with_account(500_000_00, 100_000_00)
        .build();

    broker.connect().unwrap();
    let positions = broker.positions().unwrap();
    assert_eq!(positions.len(), 2);
}

#[test]
fn mock_multiple_quotes() {
    let mut broker = MockBroker::builder()
        .with_quote(aapl(), 184_50, 185_50)
        .with_quote(msft(), 419_00, 421_00)
        .build();

    broker.connect().unwrap();
    let q1 = broker.quote(&aapl()).unwrap();
    let q2 = broker.quote(&msft()).unwrap();
    assert_eq!(q1.bid_cents, 184_50);
    assert_eq!(q2.ask_cents, 421_00);
}

#[test]
fn mock_unknown_symbol_quote() {
    let mut broker = MockBroker::builder()
        .with_quote(aapl(), 184_50, 185_50)
        .build();

    broker.connect().unwrap();
    assert!(broker.quote(&msft()).is_err());
}

#[test]
fn mock_connect_disconnect_cycle() {
    let mut broker = MockBroker::builder().build();

    // Not connected
    assert!(broker.positions().is_err());

    // Connect
    broker.connect().unwrap();
    assert!(broker.positions().is_ok());

    // Disconnect
    broker.disconnect().unwrap();
    assert!(broker.positions().is_err());

    // Reconnect
    broker.connect().unwrap();
    assert!(broker.positions().is_ok());
}

#[test]
fn mock_partial_fill_mode() {
    let mut broker = MockBroker::builder()
        .fill_mode(FillMode::ImmediatePartial(0.75))
        .build();

    broker.connect().unwrap();
    let status = broker.order_status(OrderId(1)).unwrap();
    assert_eq!(status.status, OrderState::PartiallyFilled);
    assert_eq!(status.filled_quantity, 75);
}

#[test]
fn mock_account_gross_position_value() {
    let mut broker = MockBroker::builder()
        .with_position(aapl(), 100, 185_00)
        .with_position(msft(), -50, 420_00) // short
        .with_account(1_000_000_00, 500_000_00)
        .build();

    broker.connect().unwrap();
    let acct = broker.account().unwrap();
    // gross = |100*185_00| + |-50*420_00| = 1_850_000 + 2_100_000 = 3_950_000
    assert_eq!(acct.gross_position_value_cents, 39_500_00);
}
