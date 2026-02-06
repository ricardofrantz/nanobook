//! Position tracking for a single symbol.

use crate::types::Symbol;

/// A position in a single instrument.
///
/// Tracks quantity (positive = long, negative = short), average entry price,
/// and realized PnL. All monetary values are in the smallest currency unit (cents).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Position {
    /// Symbol this position is for
    pub symbol: Symbol,
    /// Net quantity: positive = long, negative = short, zero = flat
    pub quantity: i64,
    /// Volume-weighted average entry price (cents)
    pub avg_entry_price: i64,
    /// Cumulative realized PnL (cents)
    pub realized_pnl: i64,
    /// Cumulative cost of entry (quantity * avg_entry_price), used for VWAP tracking
    pub total_cost: i64,
}

impl Position {
    /// Create a new flat position for the given symbol.
    pub fn new(symbol: Symbol) -> Self {
        Self {
            symbol,
            quantity: 0,
            avg_entry_price: 0,
            realized_pnl: 0,
            total_cost: 0,
        }
    }

    /// Apply a fill to this position.
    ///
    /// `qty` is signed: positive = buy, negative = sell.
    /// `price` is in cents (matches `Price.0`).
    ///
    /// If the fill increases the position (same direction), the average entry
    /// price is updated via VWAP. If it reduces or flips the position,
    /// realized PnL is recorded for the closed portion.
    pub fn apply_fill(&mut self, qty: i64, price: i64) {
        if qty == 0 {
            return;
        }

        let same_direction = (self.quantity >= 0 && qty > 0) || (self.quantity <= 0 && qty < 0);

        if self.quantity == 0 {
            // Opening a new position
            self.quantity = qty;
            self.avg_entry_price = price;
            self.total_cost = qty * price;
        } else if same_direction {
            // Adding to position — update VWAP
            self.total_cost += qty * price;
            self.quantity += qty;
            self.avg_entry_price = self.total_cost / self.quantity;
        } else {
            // Reducing or flipping
            let close_qty = qty.abs().min(self.quantity.abs());
            let pnl_per_unit = if self.quantity > 0 {
                price - self.avg_entry_price // long: sell higher = profit
            } else {
                self.avg_entry_price - price // short: buy lower = profit
            };
            self.realized_pnl += pnl_per_unit * close_qty;

            let net = self.quantity + qty;
            if net == 0 {
                // Fully closed
                self.quantity = 0;
                self.avg_entry_price = 0;
                self.total_cost = 0;
            } else if (net > 0) == (self.quantity > 0) {
                // Partially closed, same side — subtract closed portion's cost
                // to preserve any fractional remainder in total_cost
                self.total_cost -= close_qty * self.avg_entry_price;
                self.quantity = net;
                self.avg_entry_price = self.total_cost / self.quantity;
            } else {
                // Flipped sides
                self.quantity = net;
                self.avg_entry_price = price;
                self.total_cost = net * price;
            }
        }
    }

    /// Current market value at the given price (cents).
    #[inline]
    pub fn market_value(&self, price: i64) -> i64 {
        self.quantity * price
    }

    /// Unrealized PnL at the given market price (cents).
    #[inline]
    pub fn unrealized_pnl(&self, price: i64) -> i64 {
        if self.quantity == 0 {
            return 0;
        }
        (price - self.avg_entry_price) * self.quantity
    }

    /// Returns true if the position is flat (zero quantity).
    #[inline]
    pub fn is_flat(&self) -> bool {
        self.quantity == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sym() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn new_position_is_flat() {
        let pos = Position::new(sym());
        assert!(pos.is_flat());
        assert_eq!(pos.quantity, 0);
        assert_eq!(pos.realized_pnl, 0);
        assert_eq!(pos.unrealized_pnl(100_00), 0);
    }

    #[test]
    fn open_long() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00);
        assert_eq!(pos.quantity, 100);
        assert_eq!(pos.avg_entry_price, 50_00);
        assert_eq!(pos.market_value(55_00), 100 * 55_00);
        assert_eq!(pos.unrealized_pnl(55_00), 100 * 5_00);
    }

    #[test]
    fn add_to_long_vwap() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00); // buy 100 @ $50
        pos.apply_fill(100, 60_00); // buy 100 @ $60
        assert_eq!(pos.quantity, 200);
        assert_eq!(pos.avg_entry_price, 55_00); // VWAP
    }

    #[test]
    fn close_long_with_profit() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00); // buy 100 @ $50
        pos.apply_fill(-100, 60_00); // sell 100 @ $60
        assert!(pos.is_flat());
        assert_eq!(pos.realized_pnl, 100 * 10_00); // $10 * 100 shares
    }

    #[test]
    fn close_long_with_loss() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00); // buy 100 @ $50
        pos.apply_fill(-100, 45_00); // sell 100 @ $45
        assert!(pos.is_flat());
        assert_eq!(pos.realized_pnl, -100 * 5_00); // -$5 * 100 shares
    }

    #[test]
    fn partial_close() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00);
        pos.apply_fill(-50, 60_00); // close half
        assert_eq!(pos.quantity, 50);
        assert_eq!(pos.avg_entry_price, 50_00); // unchanged
        assert_eq!(pos.realized_pnl, 50 * 10_00);
    }

    #[test]
    fn flip_long_to_short() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00); // long 100 @ $50
        pos.apply_fill(-150, 60_00); // sell 150 — close 100, open short 50
        assert_eq!(pos.quantity, -50);
        assert_eq!(pos.avg_entry_price, 60_00);
        assert_eq!(pos.realized_pnl, 100 * 10_00); // profit on closed long
    }

    #[test]
    fn short_position() {
        let mut pos = Position::new(sym());
        pos.apply_fill(-100, 50_00); // short 100 @ $50
        assert_eq!(pos.quantity, -100);
        assert_eq!(pos.unrealized_pnl(45_00), 100 * 5_00); // profit when price drops
        assert_eq!(pos.unrealized_pnl(55_00), -100 * 5_00); // loss when price rises
    }

    #[test]
    fn close_short_with_profit() {
        let mut pos = Position::new(sym());
        pos.apply_fill(-100, 50_00); // short @ $50
        pos.apply_fill(100, 40_00); // cover @ $40
        assert!(pos.is_flat());
        assert_eq!(pos.realized_pnl, 100 * 10_00); // $10 * 100
    }

    #[test]
    fn zero_fill_is_noop() {
        let mut pos = Position::new(sym());
        pos.apply_fill(100, 50_00);
        pos.apply_fill(0, 60_00);
        assert_eq!(pos.quantity, 100);
        assert_eq!(pos.avg_entry_price, 50_00);
    }
}
