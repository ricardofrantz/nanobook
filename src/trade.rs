//! Trade representation

use crate::{OrderId, Price, Quantity, Side, Timestamp, TradeId, error::ValidationError};
use std::fmt;

/// A completed trade between two orders.
///
/// Trades are created when an incoming (aggressor) order matches
/// against a resting (passive) order on the book.
#[derive(Clone, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Trade {
    /// Unique identifier assigned by exchange
    pub id: TradeId,
    /// Execution price (always the resting order's price)
    pub price: Price,
    /// Quantity executed
    pub quantity: Quantity,
    /// Order that initiated the trade (taker)
    pub aggressor_order_id: OrderId,
    /// Order that was resting on the book (maker)
    pub passive_order_id: OrderId,
    /// Side of the aggressor order
    pub aggressor_side: Side,
    /// When the trade occurred
    pub timestamp: Timestamp,
}

impl Trade {
    /// Create a new trade.
    pub fn new(
        id: TradeId,
        price: Price,
        quantity: Quantity,
        aggressor_order_id: OrderId,
        passive_order_id: OrderId,
        aggressor_side: Side,
        timestamp: Timestamp,
    ) -> Self {
        Self {
            id,
            price,
            quantity,
            aggressor_order_id,
            passive_order_id,
            aggressor_side,
            timestamp,
        }
    }

    /// Returns the side of the passive (maker) order.
    #[inline]
    pub fn passive_side(&self) -> Side {
        self.aggressor_side.opposite()
    }

    /// Returns the notional value (price × quantity).
    ///
    /// The product is the raw `price.0 * quantity`; interpretation
    /// depends on the caller's price-unit convention.
    ///
    /// # Errors
    ///
    /// Returns [`ValidationError::NotionalOverflow`] when
    /// `price.0.checked_mul(quantity as i64)` overflows `i64`. At
    /// nanobook's default cents-as-i64 convention this requires a
    /// single trade whose notional exceeds `i64::MAX` cents
    /// (~$9.22 × 10¹⁶) — implausible in normal operation but possible
    /// with adversarial or mis-scaled inputs.
    #[inline]
    pub fn notional(&self) -> Result<i64, ValidationError> {
        checked_notional(self.price.0, self.quantity)
    }

    /// Compute the volume-weighted average price (VWAP) of a trade series.
    ///
    /// Returns `None` if the slice is empty or total quantity is zero.
    ///
    /// ```
    /// use nanobook::{Trade, Price, TradeId, OrderId, Side};
    ///
    /// let trades = vec![
    ///     Trade::new(TradeId(1), Price(100_00), 50, OrderId(1), OrderId(2), Side::Buy, 1),
    ///     Trade::new(TradeId(2), Price(102_00), 150, OrderId(3), OrderId(4), Side::Buy, 2),
    /// ];
    /// let vwap = Trade::vwap(&trades).unwrap();
    /// // (100_00 * 50 + 102_00 * 150) / 200 = 101_50
    /// assert_eq!(vwap, Price(101_50));
    /// ```
    pub fn vwap(trades: &[Trade]) -> Option<Price> {
        if trades.is_empty() {
            return None;
        }
        let total_qty: u64 = trades.iter().map(|t| t.quantity).sum();
        if total_qty == 0 {
            return None;
        }
        // Checked accumulation: either per-trade notional or the running
        // sum could exceed `i64::MAX`. `None` on overflow preserves the
        // existing `Option<Price>` signature — a degenerate sum is as
        // meaningless as an empty input.
        let mut total_notional: i64 = 0;
        for trade in trades {
            let n = checked_notional(trade.price.0, trade.quantity).ok()?;
            total_notional = total_notional.checked_add(n)?;
        }
        Some(Price(total_notional / total_qty as i64))
    }
}

/// Module-private helper: checked `price × quantity` with a uniform
/// error shape. Shared by `Trade::notional` and `Trade::vwap` so the
/// overflow semantics are identical at every site.
fn checked_notional(price: i64, quantity: u64) -> Result<i64, ValidationError> {
    // `quantity as i64` is well-defined for values ≤ i64::MAX. If a
    // quantity exceeds `i64::MAX`, the multiplication would already be
    // out of range — fold both cases into `NotionalOverflow`.
    let qty_i64 = i64::try_from(quantity)
        .map_err(|_| ValidationError::NotionalOverflow { price, quantity })?;
    price
        .checked_mul(qty_i64)
        .ok_or(ValidationError::NotionalOverflow { price, quantity })
}

impl fmt::Display for Trade {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} {} @ {} ({} aggressor)",
            self.id,
            self.quantity,
            if self.aggressor_side == Side::Buy {
                "bought"
            } else {
                "sold"
            },
            self.price,
            self.aggressor_order_id
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_trade() -> Trade {
        Trade::new(
            TradeId(1),
            Price(100_50), // $100.50
            100,
            OrderId(10), // aggressor
            OrderId(5),  // passive
            Side::Buy,
            1000,
        )
    }

    #[test]
    fn trade_creation() {
        let trade = make_trade();

        assert_eq!(trade.id, TradeId(1));
        assert_eq!(trade.price, Price(100_50));
        assert_eq!(trade.quantity, 100);
        assert_eq!(trade.aggressor_order_id, OrderId(10));
        assert_eq!(trade.passive_order_id, OrderId(5));
        assert_eq!(trade.aggressor_side, Side::Buy);
        assert_eq!(trade.timestamp, 1000);
    }

    #[test]
    fn passive_side() {
        let buy_aggressor = make_trade();
        assert_eq!(buy_aggressor.passive_side(), Side::Sell);

        let sell_aggressor = Trade::new(
            TradeId(2),
            Price(99_00),
            50,
            OrderId(11),
            OrderId(6),
            Side::Sell,
            2000,
        );
        assert_eq!(sell_aggressor.passive_side(), Side::Buy);
    }

    #[test]
    fn notional_value() {
        let trade = make_trade();
        // 10050 (cents) * 100 (shares) = 1_005_000 cent-shares
        // Interpretation: $10,050.00 notional value
        assert_eq!(trade.notional().unwrap(), 1_005_000);
    }

    /// Regression for S4: `Trade::notional` now reports overflow as an
    /// explicit error instead of silently wrapping through `i64::MIN`.
    #[test]
    fn notional_overflow_errors() {
        let trade = Trade::new(
            TradeId(1),
            Price(i64::MAX),
            2,
            OrderId(1),
            OrderId(2),
            Side::Buy,
            1,
        );
        match trade.notional() {
            Err(ValidationError::NotionalOverflow { price, quantity }) => {
                assert_eq!(price, i64::MAX);
                assert_eq!(quantity, 2);
            }
            other => panic!("expected NotionalOverflow, got {other:?}"),
        }
    }

    /// Quantity that does not fit in `i64` also routes through the
    /// overflow error rather than truncating via `as i64`.
    #[test]
    fn notional_overflow_on_huge_quantity() {
        let trade = Trade::new(
            TradeId(1),
            Price(2),
            u64::MAX,
            OrderId(1),
            OrderId(2),
            Side::Buy,
            1,
        );
        assert!(matches!(
            trade.notional(),
            Err(ValidationError::NotionalOverflow { .. })
        ));
    }

    /// A VWAP series whose intermediate notionals individually fit but
    /// whose running sum overflows must return `None`, not wrap.
    #[test]
    fn vwap_overflow_returns_none() {
        // Two trades, each notional = i64::MAX / 2 + 1; their sum
        // overflows. price.0 = i64::MAX / 2 + 1, qty = 1 → n = price.
        // Two of those summed wrap.
        let half_max_plus_one = i64::MAX / 2 + 1;
        let trades = vec![
            Trade::new(
                TradeId(1),
                Price(half_max_plus_one),
                1,
                OrderId(1),
                OrderId(2),
                Side::Buy,
                1,
            ),
            Trade::new(
                TradeId(2),
                Price(half_max_plus_one),
                1,
                OrderId(3),
                OrderId(4),
                Side::Buy,
                2,
            ),
        ];
        assert_eq!(Trade::vwap(&trades), None);
    }

    proptest::proptest! {
        #![proptest_config(proptest::test_runner::Config::with_cases(512))]

        /// S4 acceptance: `price ∈ (i64::MAX/2, i64::MAX]` paired with
        /// `qty ∈ 2..=10` overflows `i64` — the helper must surface it
        /// as `NotionalOverflow`, never panic, never wrap.
        #[test]
        fn notional_overflow_boundary(
            price in (i64::MAX / 2 + 1)..=i64::MAX,
            qty in 2u64..=10,
        ) {
            let trade = Trade::new(
                TradeId(1),
                Price(price),
                qty,
                OrderId(1),
                OrderId(2),
                Side::Buy,
                1,
            );
            match trade.notional() {
                Err(ValidationError::NotionalOverflow { price: p, quantity: q }) => {
                    proptest::prop_assert_eq!(p, price);
                    proptest::prop_assert_eq!(q, qty);
                }
                other => {
                    proptest::prop_assert!(
                        false,
                        "expected NotionalOverflow, got {:?}",
                        other,
                    );
                }
            }
        }

        /// Safe region: any non-negative price ≤ 1e9 paired with any
        /// quantity ≤ 1e9 fits comfortably in `i64` (product ≤ 1e18 <
        /// i64::MAX ≈ 9.2e18). The helper must return `Ok`.
        #[test]
        fn notional_ok_in_safe_region(
            price in 0i64..=1_000_000_000,
            qty in 0u64..=1_000_000_000,
        ) {
            let trade = Trade::new(
                TradeId(1),
                Price(price),
                qty,
                OrderId(1),
                OrderId(2),
                Side::Buy,
                1,
            );
            let n = trade.notional();
            proptest::prop_assert!(n.is_ok(), "expected Ok, got {:?}", n);
            proptest::prop_assert_eq!(n.unwrap(), price * qty as i64);
        }
    }

    #[test]
    fn display() {
        let trade = make_trade();
        let s = format!("{}", trade);
        assert!(s.contains("T1"));
        assert!(s.contains("100"));
        assert!(s.contains("bought"));
        assert!(s.contains("$100.50"));
        assert!(s.contains("O10"));
    }

    // === VWAP tests ===

    #[test]
    fn vwap_single_trade() {
        let trades = vec![make_trade()]; // 100 @ $100.50
        assert_eq!(Trade::vwap(&trades), Some(Price(100_50)));
    }

    #[test]
    fn vwap_multiple_trades() {
        let trades = vec![
            Trade::new(
                TradeId(1),
                Price(100_00),
                50,
                OrderId(1),
                OrderId(2),
                Side::Buy,
                1,
            ),
            Trade::new(
                TradeId(2),
                Price(102_00),
                150,
                OrderId(3),
                OrderId(4),
                Side::Buy,
                2,
            ),
        ];
        // (100_00 * 50 + 102_00 * 150) / 200 = (5_000_00 + 15_300_00) / 200 = 101_50
        assert_eq!(Trade::vwap(&trades), Some(Price(101_50)));
    }

    #[test]
    fn vwap_empty() {
        assert_eq!(Trade::vwap(&[]), None);
    }
}
