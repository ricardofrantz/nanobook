//! CURRENT→TARGET diff engine.
//!
//! Computes the orders needed to move from current positions to target weights.
//! Uses nanobook's Portfolio public API for equity and weight calculations,
//! but computes share diffs directly from prices and weights without mutating
//! a Portfolio instance.

use nanobook::Symbol;
use rustc_hash::FxHashMap;
use serde::Serialize;

/// A single rebalance order (computed diff).
#[derive(Debug, Clone, Serialize)]
pub struct RebalanceOrder {
    pub symbol: Symbol,
    pub action: Action,
    pub shares: i64,
    pub limit_price_cents: i64,
    pub notional_cents: i64,
    pub description: &'static str,
}

/// Trade direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Action {
    Buy,
    Sell,
    SellShort,
    BuyCover,
}

impl std::fmt::Display for Action {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Action::Buy => write!(f, "BUY"),
            Action::Sell => write!(f, "SELL"),
            Action::SellShort => write!(f, "SELL SHORT"),
            Action::BuyCover => write!(f, "BUY COVER"),
        }
    }
}

/// Current position snapshot from IBKR (or test data).
#[derive(Debug, Clone)]
pub struct CurrentPosition {
    pub symbol: Symbol,
    pub quantity: i64,
    pub avg_cost_cents: i64,
}

/// Compute rebalance orders from current state to target weights.
///
/// # Arguments
/// - `equity_cents`: Total account equity in cents
/// - `current_positions`: Current positions (symbol, qty, avg cost)
/// - `targets`: Target (symbol, weight) pairs from target.json
/// - `prices`: Current market prices (symbol, price_cents) — bid/ask midpoint
/// - `limit_offset_bps`: Offset from mid for limit price (buy at mid+offset, sell at mid-offset)
/// - `min_trade_cents`: Skip trades smaller than this notional
///
/// Positions in current but NOT in targets get closed.
pub fn compute_diff(
    equity_cents: i64,
    current_positions: &[CurrentPosition],
    targets: &[(Symbol, f64)],
    prices: &[(Symbol, i64)],
    limit_offset_bps: u32,
    min_trade_cents: i64,
) -> Vec<RebalanceOrder> {
    let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
    let target_map: FxHashMap<Symbol, f64> = targets.iter().copied().collect();
    let current_map: FxHashMap<Symbol, i64> = current_positions
        .iter()
        .map(|p| (p.symbol, p.quantity))
        .collect();

    let mut orders = Vec::new();

    // 1. Close positions not in targets
    for pos in current_positions {
        if pos.quantity == 0 {
            continue;
        }
        if target_map.contains_key(&pos.symbol) {
            continue;
        }
        if let Some(&price) = price_map.get(&pos.symbol) {
            let shares = pos.quantity.abs();
            let limit = compute_limit_price(price, pos.quantity < 0, limit_offset_bps);
            let notional = shares * price;

            let (action, desc) = if pos.quantity > 0 {
                (Action::Sell, "close long")
            } else {
                (Action::BuyCover, "close short")
            };

            orders.push(RebalanceOrder {
                symbol: pos.symbol,
                action,
                shares,
                limit_price_cents: limit,
                notional_cents: notional,
                description: desc,
            });
        }
    }

    // 2. Rebalance each target
    for &(sym, target_weight) in targets {
        let price = match price_map.get(&sym) {
            Some(&p) if p > 0 => p,
            _ => continue,
        };

        let current_qty = current_map.get(&sym).copied().unwrap_or(0);
        let current_value = current_qty * price;
        let target_value = (equity_cents as f64 * target_weight) as i64;
        let diff_value = target_value - current_value;
        let diff_qty = diff_value / price;

        if diff_qty == 0 {
            continue;
        }

        let notional = diff_qty.abs() * price;
        if notional < min_trade_cents {
            continue;
        }

        let is_buy = diff_qty > 0;
        let limit = compute_limit_price(price, !is_buy, limit_offset_bps);

        let (action, desc) = classify_trade(current_qty, diff_qty);

        orders.push(RebalanceOrder {
            symbol: sym,
            action,
            shares: diff_qty.abs(),
            limit_price_cents: limit,
            notional_cents: notional,
            description: desc,
        });
    }

    orders
}

/// Classify a trade based on current position and desired change.
fn classify_trade(current_qty: i64, diff_qty: i64) -> (Action, &'static str) {
    match (current_qty, diff_qty) {
        // No position → buying long
        (0, d) if d > 0 => (Action::Buy, "open"),
        // No position → selling short
        (0, d) if d < 0 => (Action::SellShort, "open short"),
        // Long position → buying more
        (c, d) if c > 0 && d > 0 => (Action::Buy, "increase"),
        // Long position → selling some
        (c, d) if c > 0 && d < 0 && d.abs() <= c => (Action::Sell, "decrease"),
        // Long position → selling all + going short
        (c, d) if c > 0 && d < 0 && d.abs() > c => (Action::Sell, "flip to short"),
        // Short position → covering some
        (c, d) if c < 0 && d > 0 && d <= c.abs() => (Action::BuyCover, "decrease short"),
        // Short position → covering all + going long
        (c, d) if c < 0 && d > 0 && d > c.abs() => (Action::BuyCover, "flip to long"),
        // Short position → shorting more
        (c, d) if c < 0 && d < 0 => (Action::SellShort, "increase short"),
        _ => (Action::Buy, "rebalance"),
    }
}

/// Compute limit price with offset from mid.
/// Buys get mid + offset, sells get mid - offset.
fn compute_limit_price(mid_cents: i64, is_sell: bool, offset_bps: u32) -> i64 {
    let offset = mid_cents * offset_bps as i64 / 10_000;
    if is_sell {
        mid_cents - offset
    } else {
        mid_cents + offset
    }
}

/// Estimate total execution cost for a set of orders.
pub fn estimate_cost(
    orders: &[RebalanceOrder],
    commission_per_share: f64,
    commission_min: f64,
    slippage_bps: u32,
) -> CostEstimate {
    let mut total_commission = 0.0_f64;
    let mut total_slippage = 0.0_f64;

    for order in orders {
        let commission = (order.shares as f64 * commission_per_share).max(commission_min);
        let slippage = order.notional_cents as f64 * slippage_bps as f64 / 10_000.0;
        total_commission += commission;
        total_slippage += slippage;
    }

    CostEstimate {
        commission_cents: (total_commission * 100.0) as i64,
        slippage_cents: (total_slippage) as i64,
    }
}

/// Estimated execution costs.
#[derive(Debug, Clone, Serialize)]
pub struct CostEstimate {
    pub commission_cents: i64,
    pub slippage_cents: i64,
}

impl CostEstimate {
    pub fn total_cents(&self) -> i64 {
        self.commission_cents + self.slippage_cents
    }
}

impl std::fmt::Display for CostEstimate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "${:.2} commission + ${:.2} slippage = ${:.2}",
            self.commission_cents as f64 / 100.0,
            self.slippage_cents as f64 / 100.0,
            self.total_cents() as f64 / 100.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }
    fn msft() -> Symbol {
        Symbol::new("MSFT")
    }
    fn spy() -> Symbol {
        Symbol::new("SPY")
    }

    #[test]
    fn basic_buy() {
        let orders = compute_diff(
            1_000_000_00, // $1M
            &[],          // no current positions
            &[(aapl(), 0.5)],
            &[(aapl(), 185_00)],
            5,       // 5 bps offset
            100_00,  // $100 min trade
        );

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].symbol, aapl());
        assert_eq!(orders[0].action, Action::Buy);
        assert_eq!(orders[0].description, "open");
        // $500,000 / $185 = 2702 shares
        assert_eq!(orders[0].shares, 2702);
    }

    #[test]
    fn rebalance_increase() {
        let current = vec![CurrentPosition {
            symbol: aapl(),
            quantity: 100,
            avg_cost_cents: 180_00,
        }];

        let orders = compute_diff(
            1_000_000_00,
            &current,
            &[(aapl(), 0.5)],
            &[(aapl(), 185_00)],
            0,
            0,
        );

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].action, Action::Buy);
        assert_eq!(orders[0].description, "increase");
        // Target: 2702 shares. Current: 100. Diff: 2602
        assert_eq!(orders[0].shares, 2602);
    }

    #[test]
    fn close_unwanted_position() {
        let current = vec![CurrentPosition {
            symbol: msft(),
            quantity: 200,
            avg_cost_cents: 400_00,
        }];

        let orders = compute_diff(
            1_000_000_00,
            &current,
            &[(aapl(), 0.5)], // MSFT not in targets → close
            &[(aapl(), 185_00), (msft(), 410_00)],
            0,
            0,
        );

        // Should have 2 orders: close MSFT + open AAPL
        assert_eq!(orders.len(), 2);
        let close = orders.iter().find(|o| o.symbol == msft()).unwrap();
        assert_eq!(close.action, Action::Sell);
        assert_eq!(close.shares, 200);
        assert_eq!(close.description, "close long");
    }

    #[test]
    fn short_position() {
        let orders = compute_diff(
            1_000_000_00,
            &[],
            &[(spy(), -0.10)], // 10% short
            &[(spy(), 430_00)],
            0,
            0,
        );

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].action, Action::SellShort);
        assert_eq!(orders[0].description, "open short");
        // -$100,000 / $430 = 232 shares
        assert_eq!(orders[0].shares, 232);
    }

    #[test]
    fn skip_tiny_trade() {
        let current = vec![CurrentPosition {
            symbol: aapl(),
            quantity: 2702, // already at target
            avg_cost_cents: 185_00,
        }];

        let orders = compute_diff(
            1_000_000_00,
            &current,
            &[(aapl(), 0.5)],
            &[(aapl(), 185_00)],
            0,
            100_00, // $100 min trade
        );

        // Diff is 0 shares (or very small) → no orders
        assert!(orders.is_empty());
    }

    #[test]
    fn limit_price_offset() {
        let orders = compute_diff(
            1_000_000_00,
            &[],
            &[(aapl(), 0.5)],
            &[(aapl(), 200_00)], // $200
            10,                   // 10 bps
            0,
        );

        assert_eq!(orders.len(), 1);
        // Buy: mid + 10bps = $200.00 + $0.20 = $200.20
        assert_eq!(orders[0].limit_price_cents, 200_20);
    }

    #[test]
    fn cost_estimation() {
        let orders = compute_diff(
            1_000_000_00,
            &[],
            &[(aapl(), 0.5)],
            &[(aapl(), 185_00)],
            0,
            0,
        );

        let cost = estimate_cost(&orders, 0.0035, 0.35, 5);
        assert!(cost.commission_cents > 0);
        assert!(cost.slippage_cents > 0);
        assert!(cost.total_cents() > 0);
    }

    #[test]
    fn close_short_position() {
        let current = vec![CurrentPosition {
            symbol: spy(),
            quantity: -100, // short 100 shares
            avg_cost_cents: 430_00,
        }];

        let orders = compute_diff(
            1_000_000_00,
            &current,
            &[], // no targets → close everything
            &[(spy(), 430_00)],
            0,
            0,
        );

        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].action, Action::BuyCover);
        assert_eq!(orders[0].shares, 100);
        assert_eq!(orders[0].description, "close short");
    }
}
