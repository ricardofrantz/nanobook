//! Transaction cost modeling.

/// Models transaction costs for portfolio rebalancing.
///
/// Costs are computed as a percentage of notional value (in basis points)
/// plus a minimum per-trade fee.
///
/// ```ignore
/// use nanobook::portfolio::CostModel;
///
/// let model = CostModel { commission_bps: 10, slippage_bps: 5, min_trade_fee: 1_00 };
/// // 15 bps on $10,000 notional = $1.50, but min fee is $1.00, so result = $1.50
/// assert_eq!(model.compute_cost(1_000_000), 1500);
/// ```
#[derive(Clone, Copy, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct CostModel {
    /// Commission in basis points (1 bps = 0.01%)
    pub commission_bps: u32,
    /// Slippage estimate in basis points
    pub slippage_bps: u32,
    /// Minimum fee per trade (cents)
    pub min_trade_fee: i64,
}

impl CostModel {
    /// A zero-cost model (no fees, no slippage).
    pub fn zero() -> Self {
        Self {
            commission_bps: 0,
            slippage_bps: 0,
            min_trade_fee: 0,
        }
    }

    /// Compute the total cost for a trade with the given absolute notional value (cents).
    ///
    /// The notional should be `|quantity * price|`. Returns the cost in cents,
    /// which is always non-negative.
    pub fn compute_cost(&self, notional: i64) -> i64 {
        let notional = notional.unsigned_abs();
        let total_bps = self.commission_bps as u64 + self.slippage_bps as u64;
        // notional * bps / 10_000
        let bps_cost = (notional * total_bps / 10_000) as i64;
        bps_cost.max(self.min_trade_fee)
    }
}

impl Default for CostModel {
    fn default() -> Self {
        Self::zero()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_cost() {
        let model = CostModel::zero();
        assert_eq!(model.compute_cost(1_000_000), 0);
    }

    #[test]
    fn bps_cost() {
        let model = CostModel {
            commission_bps: 10,
            slippage_bps: 5,
            min_trade_fee: 0,
        };
        // 15 bps on 1_000_000 cents ($10,000) = 1500 cents ($15)
        assert_eq!(model.compute_cost(1_000_000), 1500);
    }

    #[test]
    fn min_fee_applied() {
        let model = CostModel {
            commission_bps: 1,
            slippage_bps: 0,
            min_trade_fee: 1_00, // $1 minimum
        };
        // 1 bps on 10_000 cents ($100) = 1 cent, but min is $1.00
        assert_eq!(model.compute_cost(10_000), 1_00);
    }

    #[test]
    fn negative_notional_uses_abs() {
        let model = CostModel {
            commission_bps: 10,
            slippage_bps: 0,
            min_trade_fee: 0,
        };
        assert_eq!(model.compute_cost(-1_000_000), model.compute_cost(1_000_000));
    }

    #[test]
    fn cost_always_non_negative() {
        let model = CostModel::zero();
        assert!(model.compute_cost(0) >= 0);
        assert!(model.compute_cost(-100) >= 0);
    }
}
