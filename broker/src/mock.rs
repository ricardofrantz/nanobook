//! Mock broker for testing — implements the `Broker` trait with configurable behavior.
//!
//! Use this in integration tests to simulate broker responses without network calls.
//!
//! ```ignore
//! use nanobook_broker::mock::{MockBroker, FillMode};
//! use nanobook::Symbol;
//!
//! let broker = MockBroker::builder()
//!     .fill_mode(FillMode::ImmediateFull)
//!     .with_position(Symbol::new("AAPL"), 100, 150_00)
//!     .with_account(1_000_000_00, 500_000_00)
//!     .build();
//! ```

use std::sync::Mutex;

use nanobook::Symbol;

use crate::error::BrokerError;
use crate::types::*;
use crate::Broker;

/// How the mock broker handles submitted orders.
#[derive(Clone, Debug)]
pub enum FillMode {
    /// Orders are immediately fully filled at the limit price (or mid for market).
    ImmediateFull,
    /// Orders are partially filled (the given fraction, e.g., 0.5 = 50%).
    ImmediatePartial(f64),
    /// All orders are rejected.
    Reject,
}

/// A recorded order submission for assertion in tests.
#[derive(Clone, Debug)]
pub struct RecordedOrder {
    pub symbol: Symbol,
    pub side: BrokerSide,
    pub quantity: u64,
    pub order_type: String,
}

/// Builder for `MockBroker`.
pub struct MockBrokerBuilder {
    fill_mode: FillMode,
    positions: Vec<Position>,
    quotes: Vec<(Symbol, Quote)>,
    equity_cents: i64,
    cash_cents: i64,
}

impl MockBrokerBuilder {
    pub fn fill_mode(mut self, mode: FillMode) -> Self {
        self.fill_mode = mode;
        self
    }

    pub fn with_position(mut self, symbol: Symbol, quantity: i64, avg_cost_cents: i64) -> Self {
        let market_value = quantity * avg_cost_cents;
        self.positions.push(Position {
            symbol,
            quantity,
            avg_cost_cents,
            market_value_cents: market_value,
            unrealized_pnl_cents: 0,
        });
        self
    }

    pub fn with_quote(mut self, symbol: Symbol, bid: i64, ask: i64) -> Self {
        self.quotes.push((
            symbol,
            Quote {
                symbol,
                bid_cents: bid,
                ask_cents: ask,
                last_cents: (bid + ask) / 2,
                volume: 0,
            },
        ));
        self
    }

    pub fn with_account(mut self, equity_cents: i64, cash_cents: i64) -> Self {
        self.equity_cents = equity_cents;
        self.cash_cents = cash_cents;
        self
    }

    pub fn build(self) -> MockBroker {
        MockBroker {
            connected: false,
            fill_mode: self.fill_mode,
            positions: self.positions,
            quotes: self.quotes,
            equity_cents: self.equity_cents,
            cash_cents: self.cash_cents,
            next_order_id: 1,
            submitted_orders: Mutex::new(Vec::new()),
        }
    }
}

/// A mock broker that records submitted orders and returns configurable responses.
pub struct MockBroker {
    connected: bool,
    fill_mode: FillMode,
    positions: Vec<Position>,
    quotes: Vec<(Symbol, Quote)>,
    equity_cents: i64,
    cash_cents: i64,
    next_order_id: u64,
    submitted_orders: Mutex<Vec<RecordedOrder>>,
}

impl MockBroker {
    pub fn builder() -> MockBrokerBuilder {
        MockBrokerBuilder {
            fill_mode: FillMode::ImmediateFull,
            positions: Vec::new(),
            quotes: Vec::new(),
            equity_cents: 100_000_000,
            cash_cents: 100_000_000,
        }
    }

    /// Get all orders that were submitted (for assertion in tests).
    pub fn submitted_orders(&self) -> Vec<RecordedOrder> {
        self.submitted_orders.lock().unwrap().clone()
    }
}

impl Broker for MockBroker {
    fn connect(&mut self) -> Result<(), BrokerError> {
        self.connected = true;
        Ok(())
    }

    fn disconnect(&mut self) -> Result<(), BrokerError> {
        self.connected = false;
        Ok(())
    }

    fn positions(&self) -> Result<Vec<Position>, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        Ok(self.positions.clone())
    }

    fn account(&self) -> Result<Account, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        let gross = self
            .positions
            .iter()
            .map(|p| p.market_value_cents.abs())
            .sum();
        Ok(Account {
            equity_cents: self.equity_cents,
            buying_power_cents: self.cash_cents,
            cash_cents: self.cash_cents,
            gross_position_value_cents: gross,
        })
    }

    fn submit_order(&self, order: &BrokerOrder) -> Result<OrderId, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        // Record the order
        self.submitted_orders.lock().unwrap().push(RecordedOrder {
            symbol: order.symbol,
            side: order.side,
            quantity: order.quantity,
            order_type: format!("{:?}", order.order_type),
        });

        match &self.fill_mode {
            FillMode::Reject => Err(BrokerError::Order("mock: order rejected".into())),
            _ => Ok(OrderId(self.next_order_id)),
        }
    }

    fn order_status(&self, id: OrderId) -> Result<BrokerOrderStatus, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }

        // Return status based on fill mode
        let (status, filled, remaining) = match &self.fill_mode {
            FillMode::ImmediateFull => (OrderState::Filled, 100, 0),
            FillMode::ImmediatePartial(frac) => {
                let filled = (100.0 * frac) as u64;
                (OrderState::PartiallyFilled, filled, 100 - filled)
            }
            FillMode::Reject => (OrderState::Rejected, 0, 0),
        };

        Ok(BrokerOrderStatus {
            id,
            status,
            filled_quantity: filled,
            remaining_quantity: remaining,
            avg_fill_price_cents: 0,
        })
    }

    fn cancel_order(&self, _id: OrderId) -> Result<(), BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        Ok(())
    }

    fn quote(&self, symbol: &Symbol) -> Result<Quote, BrokerError> {
        if !self.connected {
            return Err(BrokerError::NotConnected);
        }
        self.quotes
            .iter()
            .find(|(s, _)| s == symbol)
            .map(|(_, q)| q.clone())
            .ok_or_else(|| BrokerError::InvalidSymbol(symbol.as_str().to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanobook::Price;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn builder_basic() {
        let mut broker = MockBroker::builder()
            .with_position(aapl(), 100, 150_00)
            .with_account(1_000_000_00, 500_000_00)
            .with_quote(aapl(), 149_50, 150_50)
            .build();

        broker.connect().unwrap();

        let positions = broker.positions().unwrap();
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].symbol, aapl());
        assert_eq!(positions[0].quantity, 100);

        let account = broker.account().unwrap();
        assert_eq!(account.equity_cents, 1_000_000_00);

        let quote = broker.quote(&aapl()).unwrap();
        assert_eq!(quote.bid_cents, 149_50);
        assert_eq!(quote.ask_cents, 150_50);
    }

    #[test]
    fn not_connected_errors() {
        let broker = MockBroker::builder().build();
        assert!(broker.positions().is_err());
        assert!(broker.account().is_err());
    }

    #[test]
    fn submit_records_orders() {
        let mut broker = MockBroker::builder().build();
        broker.connect().unwrap();

        let order = BrokerOrder {
            symbol: aapl(),
            side: BrokerSide::Buy,
            quantity: 50,
            order_type: BrokerOrderType::Limit(Price(150_00)),
        };

        let id = broker.submit_order(&order).unwrap();
        assert_eq!(id, OrderId(1));

        let recorded = broker.submitted_orders();
        assert_eq!(recorded.len(), 1);
        assert_eq!(recorded[0].symbol, aapl());
        assert_eq!(recorded[0].quantity, 50);
    }

    #[test]
    fn reject_mode() {
        let mut broker = MockBroker::builder().fill_mode(FillMode::Reject).build();
        broker.connect().unwrap();

        let order = BrokerOrder {
            symbol: aapl(),
            side: BrokerSide::Buy,
            quantity: 50,
            order_type: BrokerOrderType::Market,
        };

        assert!(broker.submit_order(&order).is_err());
    }

    #[test]
    fn partial_fill_status() {
        let mut broker = MockBroker::builder()
            .fill_mode(FillMode::ImmediatePartial(0.5))
            .build();
        broker.connect().unwrap();

        let status = broker.order_status(OrderId(1)).unwrap();
        assert_eq!(status.status, OrderState::PartiallyFilled);
        assert_eq!(status.filled_quantity, 50);
    }
}
