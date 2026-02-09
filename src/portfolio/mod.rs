//! Portfolio management: position tracking, cost modeling, and financial metrics.
//!
//! The portfolio layer sits on top of the LOB infrastructure. It supports two
//! execution modes:
//!
//! - **SimpleFill**: Instant execution at specified prices (for fast parameter sweeps)
//! - **LOBFill**: Route orders through actual `Exchange` matching engines (for microstructure)
//!
//! # Example
//!
//! ```ignore
//! use nanobook::portfolio::{Portfolio, CostModel};
//! use nanobook::Symbol;
//!
//! let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero()); // $1M
//!
//! // Rebalance to 60% AAPL, 40% MSFT at current prices
//! let targets = [(Symbol::new("AAPL"), 0.6), (Symbol::new("MSFT"), 0.4)];
//! let prices = [(Symbol::new("AAPL"), 150_00), (Symbol::new("MSFT"), 300_00)];
//! portfolio.rebalance_simple(&targets, &prices);
//! ```

pub mod cost_model;
pub mod metrics;
pub mod position;
pub mod strategy;
#[cfg(feature = "parallel")]
pub mod sweep;

pub use cost_model::CostModel;
pub use metrics::{Metrics, compute_metrics};
pub use position::Position;
pub use strategy::{BacktestResult, EqualWeight, Strategy, run_backtest};

use crate::types::Symbol;
use rustc_hash::FxHashMap;

/// Serde helper for `FxHashMap<Symbol, Position>` — serializes as `Vec<(Symbol, Position)>`.
#[cfg(feature = "serde")]
mod serde_positions {
    use super::{FxHashMap, Position, Symbol};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S: Serializer>(
        map: &FxHashMap<Symbol, Position>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mut vec: Vec<(&Symbol, &Position)> = map.iter().collect();
        vec.sort_by_key(|(sym, _)| *sym);
        vec.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<FxHashMap<Symbol, Position>, D::Error> {
        let vec: Vec<(Symbol, Position)> = Vec::deserialize(deserializer)?;
        Ok(vec.into_iter().collect())
    }
}

/// A portfolio tracking cash, positions, returns, and equity.
///
/// All monetary values (cash, equity) are in the smallest currency unit (cents).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Portfolio {
    /// Cash balance (cents)
    cash: i64,
    /// Positions indexed by symbol
    #[cfg_attr(
        feature = "serde",
        serde(
            serialize_with = "serde_positions::serialize",
            deserialize_with = "serde_positions::deserialize"
        )
    )]
    positions: FxHashMap<Symbol, Position>,
    /// Cost model applied to each trade
    cost_model: CostModel,
    /// Series of periodic returns (for metrics computation)
    returns: Vec<f64>,
    /// Equity curve (total portfolio value at each snapshot)
    equity_curve: Vec<i64>,
    /// Previous equity for return calculation
    prev_equity: i64,
}

impl Portfolio {
    /// Create a new portfolio with initial cash and cost model.
    ///
    /// `initial_cash` is in cents (e.g., `1_000_000_00` = $1,000,000).
    /// Negative initial cash is a programming error (use `debug_assert`).
    pub fn new(initial_cash: i64, cost_model: CostModel) -> Self {
        debug_assert!(
            initial_cash >= 0,
            "initial_cash must be non-negative, got {initial_cash}"
        );
        Self {
            cash: initial_cash,
            positions: FxHashMap::default(),
            cost_model,
            returns: Vec::new(),
            equity_curve: vec![initial_cash],
            prev_equity: initial_cash,
        }
    }

    // === Queries ===

    /// Current cash balance (cents).
    #[inline]
    pub fn cash(&self) -> i64 {
        self.cash
    }

    /// Get a position by symbol, if it exists.
    pub fn position(&self, symbol: &Symbol) -> Option<&Position> {
        self.positions.get(symbol)
    }

    /// Iterator over all positions.
    pub fn positions(&self) -> impl Iterator<Item = (&Symbol, &Position)> {
        self.positions.iter()
    }

    /// Total equity: cash + sum of all position market values.
    ///
    /// `prices` maps symbols to current prices (cents).
    pub fn total_equity(&self, prices: &[(Symbol, i64)]) -> i64 {
        let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
        let position_value: i64 = self
            .positions
            .iter()
            .map(|(sym, pos)| {
                let price = price_map.get(sym).copied().unwrap_or(0);
                pos.market_value(price)
            })
            .sum();
        self.cash + position_value
    }

    /// Current portfolio weights as (symbol, weight) pairs.
    ///
    /// Weights are fractions of total equity. Cash is not included
    /// (it's implicitly `1 - sum(weights)`).
    pub fn current_weights(&self, prices: &[(Symbol, i64)]) -> Vec<(Symbol, f64)> {
        let equity = self.total_equity(prices);
        if equity == 0 {
            return Vec::new();
        }
        let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
        self.positions
            .iter()
            .filter(|(_, pos)| !pos.is_flat())
            .map(|(sym, pos)| {
                let price = price_map.get(sym).copied().unwrap_or(0);
                let mv = pos.market_value(price) as f64;
                (*sym, mv / equity as f64)
            })
            .collect()
    }

    /// The accumulated return series.
    pub fn returns(&self) -> &[f64] {
        &self.returns
    }

    /// The equity curve (one entry per `record_return` call).
    pub fn equity_curve(&self) -> &[i64] {
        &self.equity_curve
    }

    /// The cost model in use.
    pub fn cost_model(&self) -> &CostModel {
        &self.cost_model
    }

    // === Execution ===

    /// Rebalance the portfolio to target weights using simple fill (instant execution).
    ///
    /// This is the hot path for parameter sweeps. Orders execute at the provided
    /// bar prices with no market microstructure simulation.
    ///
    /// `targets`: desired (symbol, weight) pairs. Weights should sum to ≤ 1.0.
    /// `prices`: current (symbol, price_in_cents) for each symbol.
    ///
    /// Positions not in `targets` are closed. Costs are deducted from cash.
    pub fn rebalance_simple(&mut self, targets: &[(Symbol, f64)], prices: &[(Symbol, i64)]) {
        let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
        let equity = self.total_equity(prices);
        if equity <= 0 {
            return;
        }

        let target_map: FxHashMap<Symbol, f64> = targets.iter().copied().collect();

        // Close positions not in targets
        let to_close: Vec<Symbol> = self
            .positions
            .keys()
            .filter(|sym| !target_map.contains_key(sym))
            .copied()
            .collect();

        for sym in to_close {
            if let Some(price) = price_map.get(&sym).copied() {
                let qty = match self.positions.get(&sym) {
                    Some(pos) if !pos.is_flat() => -pos.quantity,
                    _ => continue,
                };
                self.execute_fill(sym, qty, price);
            }
        }

        // Rebalance each target
        for &(sym, target_weight) in targets {
            let price = match price_map.get(&sym).copied() {
                Some(p) if p > 0 => p,
                _ => continue,
            };

            let current_value = self
                .positions
                .get(&sym)
                .map(|p| p.market_value(price))
                .unwrap_or(0);

            let target_value = (equity as f64 * target_weight) as i64;
            let diff_value = target_value - current_value;

            // Convert value difference to shares
            let diff_qty = diff_value / price;
            if diff_qty != 0 {
                self.execute_fill(sym, diff_qty, price);
            }
        }
    }

    /// Rebalance the portfolio through LOB matching engines.
    ///
    /// Routes orders through actual `Exchange` instances for realistic
    /// microstructure simulation including partial fills and price impact.
    ///
    /// `targets`: desired (symbol, weight) pairs.
    /// `exchanges`: mutable reference to a `MultiExchange` containing per-symbol LOBs.
    pub fn rebalance_lob(
        &mut self,
        targets: &[(Symbol, f64)],
        exchanges: &mut crate::multi_exchange::MultiExchange,
    ) {
        // Collect current prices from exchange BBO
        let prices: Vec<(Symbol, i64)> = exchanges
            .symbols()
            .filter_map(|sym| {
                let ex = exchanges.get(sym)?;
                let mid = {
                    let (bid, ask) = ex.best_bid_ask();
                    match (bid, ask) {
                        (Some(b), Some(a)) => b.0 + (a.0 - b.0) / 2,
                        (Some(b), None) => b.0,
                        (None, Some(a)) => a.0,
                        (None, None) => return None,
                    }
                };
                Some((*sym, mid))
            })
            .collect();

        let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
        let equity = self.total_equity(&prices);
        if equity <= 0 {
            return;
        }

        let target_map: FxHashMap<Symbol, f64> = targets.iter().copied().collect();

        // Close positions not in targets
        let to_close: Vec<Symbol> = self
            .positions
            .keys()
            .filter(|sym| !target_map.contains_key(sym))
            .copied()
            .collect();

        for sym in to_close {
            let (qty, side) = match self.positions.get(&sym) {
                Some(pos) if !pos.is_flat() => {
                    let side = if pos.quantity > 0 {
                        crate::Side::Sell
                    } else {
                        crate::Side::Buy
                    };
                    (pos.quantity.unsigned_abs(), side)
                }
                _ => continue,
            };
            let exchange = exchanges.get_or_create(&sym);
            let result = exchange.submit_market(side, qty);
            for trade in &result.trades {
                let fill_qty = if side == crate::Side::Sell {
                    -(trade.quantity as i64)
                } else {
                    trade.quantity as i64
                };
                self.execute_fill(sym, fill_qty, trade.price.0);
            }
        }

        // Rebalance each target
        for &(sym, target_weight) in targets {
            let price = match price_map.get(&sym).copied() {
                Some(p) if p > 0 => p,
                _ => continue,
            };

            let current_value = self
                .positions
                .get(&sym)
                .map(|p| p.market_value(price))
                .unwrap_or(0);

            let target_value = (equity as f64 * target_weight) as i64;
            let diff_value = target_value - current_value;
            let diff_qty = (diff_value / price).unsigned_abs();

            if diff_qty == 0 {
                continue;
            }

            let side = if diff_value > 0 {
                crate::Side::Buy
            } else {
                crate::Side::Sell
            };

            let exchange = exchanges.get_or_create(&sym);
            let result = exchange.submit_market(side, diff_qty);
            for trade in &result.trades {
                let fill_qty = if side == crate::Side::Buy {
                    trade.quantity as i64
                } else {
                    -(trade.quantity as i64)
                };
                self.execute_fill(sym, fill_qty, trade.price.0);
            }
        }
    }

    /// Record a return for the current period.
    ///
    /// Call this at the end of each period (day, month, etc.) after rebalancing.
    /// `prices` are current market prices for computing equity.
    pub fn record_return(&mut self, prices: &[(Symbol, i64)]) {
        let equity = self.total_equity(prices);
        if self.prev_equity > 0 {
            let ret = (equity - self.prev_equity) as f64 / self.prev_equity as f64;
            self.returns.push(ret);
        }
        self.equity_curve.push(equity);
        self.prev_equity = equity;
    }

    /// Take a snapshot of the portfolio state.
    pub fn snapshot(&self, prices: &[(Symbol, i64)]) -> PortfolioSnapshot {
        let equity = self.total_equity(prices);
        let weights = self.current_weights(prices);
        let total_realized_pnl: i64 = self.positions.values().map(|p| p.realized_pnl).sum();

        PortfolioSnapshot {
            cash: self.cash,
            equity,
            weights,
            num_positions: self.positions.values().filter(|p| !p.is_flat()).count(),
            total_realized_pnl,
        }
    }

    // === Persistence ===

    /// Save the portfolio to a JSON file.
    #[cfg(feature = "persistence")]
    pub fn save_json(&self, path: &std::path::Path) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        std::fs::write(path, json)
    }

    /// Load a portfolio from a JSON file.
    #[cfg(feature = "persistence")]
    pub fn load_json(path: &std::path::Path) -> std::io::Result<Self> {
        let json = std::fs::read_to_string(path)?;
        serde_json::from_str(&json).map_err(std::io::Error::other)
    }

    // === Internal ===

    /// Execute a fill: update position, deduct cost, adjust cash.
    fn execute_fill(&mut self, symbol: Symbol, qty: i64, price: i64) {
        if qty == 0 {
            return;
        }

        let notional = qty.saturating_abs().saturating_mul(price);
        let cost = self.cost_model.compute_cost(notional);

        // Update position
        let pos = self
            .positions
            .entry(symbol)
            .or_insert_with(|| Position::new(symbol));
        pos.apply_fill(qty, price);

        // Adjust cash: buying decreases cash, selling increases it
        self.cash = self.cash.saturating_sub(qty.saturating_mul(price).saturating_add(cost));
    }
}

/// A point-in-time snapshot of portfolio state.
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct PortfolioSnapshot {
    /// Cash balance (cents)
    pub cash: i64,
    /// Total equity (cents)
    pub equity: i64,
    /// Current weights
    pub weights: Vec<(Symbol, f64)>,
    /// Number of non-flat positions
    pub num_positions: usize,
    /// Total realized PnL across all positions
    pub total_realized_pnl: i64,
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

    #[test]
    fn new_portfolio() {
        let portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        assert_eq!(portfolio.cash(), 1_000_000_00);
        assert_eq!(portfolio.total_equity(&[]), 1_000_000_00);
    }

    #[test]
    fn simple_buy() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let targets = [(aapl(), 0.5)];
        let prices = [(aapl(), 150_00)];

        portfolio.rebalance_simple(&targets, &prices);

        let pos = portfolio.position(&aapl()).unwrap();
        assert!(pos.quantity > 0);
        // Should have bought ~$500,000 worth at $150 = ~3333 shares
        assert_eq!(pos.quantity, 3333);
    }

    #[test]
    fn equity_conservation_no_cost() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 150_00), (msft(), 300_00)];
        let targets = [(aapl(), 0.6), (msft(), 0.4)];

        let equity_before = portfolio.total_equity(&prices);
        portfolio.rebalance_simple(&targets, &prices);
        let equity_after = portfolio.total_equity(&prices);

        // With zero cost and integer rounding, equity should be very close
        let diff = (equity_after - equity_before).abs();
        // Allow rounding error of up to 1 share per position * max price
        assert!(diff < 2 * 300_00, "equity diff too large: {diff}");
    }

    #[test]
    fn cost_model_deducts_fees() {
        let model = CostModel {
            commission_bps: 10,
            slippage_bps: 0,
            min_trade_fee: 0,
        };
        let mut portfolio = Portfolio::new(1_000_000_00, model);
        let prices = [(aapl(), 150_00)];
        let targets = [(aapl(), 0.5)];

        portfolio.rebalance_simple(&targets, &prices);

        let equity = portfolio.total_equity(&prices);
        // Equity should be less than initial due to costs
        assert!(equity < 1_000_000_00);
    }

    #[test]
    fn rebalance_closes_unneeded_positions() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 150_00), (msft(), 300_00)];

        // First: buy AAPL and MSFT
        portfolio.rebalance_simple(&[(aapl(), 0.5), (msft(), 0.5)], &prices);
        assert!(portfolio.position(&aapl()).unwrap().quantity > 0);
        assert!(portfolio.position(&msft()).unwrap().quantity > 0);

        // Second: only AAPL — MSFT should be closed
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);
        assert!(portfolio.position(&msft()).unwrap().is_flat());
    }

    #[test]
    fn record_return_tracks_equity() {
        let mut portfolio = Portfolio::new(100_00, CostModel::zero());
        let prices = [(aapl(), 10_00)];

        portfolio.rebalance_simple(&[(aapl(), 1.0)], &prices);

        // Price goes up 10%
        let new_prices = [(aapl(), 11_00)];
        portfolio.record_return(&new_prices);

        assert_eq!(portfolio.returns().len(), 1);
        let ret = portfolio.returns()[0];
        assert!(ret > 0.0);
    }

    #[test]
    fn snapshot() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 150_00)];
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);

        let snap = portfolio.snapshot(&prices);
        assert_eq!(snap.num_positions, 1);
        // Equity should be close to initial (zero cost)
        assert!((snap.equity - 1_000_000_00).abs() < 300_00);
    }

    #[test]
    fn current_weights() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 150_00)];
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);

        let weights = portfolio.current_weights(&prices);
        assert_eq!(weights.len(), 1);
        // Weight should be approximately 0.5
        assert!((weights[0].1 - 0.5).abs() < 0.01);
    }
}

#[cfg(all(test, feature = "persistence"))]
mod persistence_tests {
    use super::*;

    fn aapl() -> Symbol {
        Symbol::new("AAPL")
    }

    #[test]
    fn portfolio_json_roundtrip() {
        let mut portfolio = Portfolio::new(1_000_000_00, CostModel::zero());
        let prices = [(aapl(), 150_00)];
        portfolio.rebalance_simple(&[(aapl(), 0.5)], &prices);
        portfolio.record_return(&prices);

        let json = serde_json::to_string(&portfolio).unwrap();
        let restored: Portfolio = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.cash(), portfolio.cash());
        assert_eq!(restored.returns().len(), portfolio.returns().len());
        assert_eq!(
            restored.position(&aapl()).unwrap().quantity,
            portfolio.position(&aapl()).unwrap().quantity
        );
    }

    #[test]
    fn portfolio_save_load_file() {
        let mut portfolio = Portfolio::new(500_000_00, CostModel::zero());
        let prices = [(aapl(), 100_00)];
        portfolio.rebalance_simple(&[(aapl(), 1.0)], &prices);

        let dir = std::env::temp_dir();
        let path = dir.join("nanobook_test_portfolio.json");

        portfolio.save_json(&path).unwrap();
        let loaded = Portfolio::load_json(&path).unwrap();

        assert_eq!(loaded.cash(), portfolio.cash());
        assert_eq!(
            loaded.position(&aapl()).unwrap().quantity,
            portfolio.position(&aapl()).unwrap().quantity
        );

        // Cleanup
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn metrics_serde() {
        let returns = vec![0.01, -0.005, 0.02];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();

        let json = serde_json::to_string(&m).unwrap();
        let restored: Metrics = serde_json::from_str(&json).unwrap();

        assert!((restored.total_return - m.total_return).abs() < 1e-10);
        assert!((restored.sharpe - m.sharpe).abs() < 1e-10);
    }
}
