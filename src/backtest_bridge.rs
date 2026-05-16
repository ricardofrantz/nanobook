//! Fast backtest bridge: simulate portfolio returns from a pre-computed weight schedule.
//!
//! Python computes the weight schedule (factor models, signals, etc.),
//! Rust handles the inner simulation loop (rebalance, track positions, compute returns).

use std::collections::{HashMap, HashSet};

use crate::portfolio::metrics::{
    DrawdownEvent, Metrics, compute_metrics, drawdown_series, rolling_sharpe,
};
use crate::portfolio::{CostModel, Portfolio};
use crate::types::Symbol;

/// Optional stop simulation configuration.
#[derive(Clone, Debug, Default)]
pub struct BacktestStopConfig {
    /// Fixed stop distance as fraction of entry price (e.g. 0.10 = 10%).
    pub fixed_stop_pct: Option<f64>,
    /// Trailing stop distance as fraction from watermark (e.g. 0.05 = 5%).
    pub trailing_stop_pct: Option<f64>,
    /// ATR multiple for adaptive trailing stop.
    pub atr_multiple: Option<f64>,
    /// Rolling period for ATR approximation (absolute close-to-close changes).
    pub atr_period: usize,
}

impl BacktestStopConfig {
    fn sanitized(&self) -> Option<Self> {
        let fixed = sanitize_pct(self.fixed_stop_pct);
        let trailing = sanitize_pct(self.trailing_stop_pct);
        let atr_multiple = sanitize_positive(self.atr_multiple);
        let atr_period = self.atr_period.max(1);

        if fixed.is_none() && trailing.is_none() && atr_multiple.is_none() {
            return None;
        }

        Some(Self {
            fixed_stop_pct: fixed,
            trailing_stop_pct: trailing,
            atr_multiple,
            atr_period,
        })
    }
}

/// Backtest options for v0.9 API surface.
#[derive(Clone, Debug, Default)]
pub struct BacktestBridgeOptions {
    /// Optional stop simulation configuration.
    pub stop_cfg: Option<BacktestStopConfig>,
}

/// Stop event emitted by stop-aware backtest simulation.
#[derive(Clone, Debug)]
pub struct BacktestStopEvent {
    /// Period index where the stop triggered.
    pub period_index: usize,
    /// Symbol that was exited.
    pub symbol: Symbol,
    /// Stop threshold that was breached.
    pub trigger_price: i64,
    /// Executed exit price.
    pub exit_price: i64,
    /// Trigger reason: `fixed`, `trailing`, `atr`.
    pub reason: &'static str,
}

/// Trade lifecycle detected from target-weight transitions.
#[derive(Debug, Clone, PartialEq)]
pub struct AttributionTrade {
    pub symbol: Symbol,
    pub entry_index: usize,
    pub exit_index: Option<usize>,
    pub entry_weight: f64,
    pub exit_weight: f64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct AttributionResult {
    pub contributions: Vec<Vec<(Symbol, f64)>>,
    pub cumulative_contributions: Vec<Vec<(Symbol, f64)>>,
    pub trades: Vec<AttributionTrade>,
}

/// Aggregate trade lifecycle counts for reporting.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeAnalytics {
    pub trade_count: usize,
    pub open_trade_count: usize,
    pub closed_trade_count: usize,
}

#[derive(Debug, Clone)]
pub struct TearSheet {
    pub monthly_returns: Vec<Vec<f64>>,
    pub rolling_sharpe: Vec<f64>,
    pub drawdown_events: Vec<DrawdownEvent>,
    pub trade_analytics: TradeAnalytics,
}

pub struct BacktestBridgeResult {
    /// Per-period returns.
    pub returns: Vec<f64>,
    /// Equity curve (one entry per date + initial equity).
    pub equity_curve: Vec<i64>,
    /// Final portfolio state.
    pub final_cash: i64,
    /// Computed metrics (None if no returns).
    pub metrics: Option<Metrics>,
    /// Per-period holdings as (symbol, weight).
    pub holdings: Vec<Vec<(Symbol, f64)>>,
    /// Per-period per-symbol close-to-close returns.
    pub symbol_returns: Vec<Vec<(Symbol, f64)>>,
    /// Stop-trigger events (empty when stop simulation disabled or no triggers).
    pub stop_events: Vec<BacktestStopEvent>,
}

/// Simulate portfolio returns from a pre-computed weight schedule.
///
/// Compatibility wrapper (v0.7/v0.8 behavior): stop simulation disabled.
pub fn backtest_weights(
    weight_schedule: &[Vec<(Symbol, f64)>],
    price_schedule: &[Vec<(Symbol, i64)>],
    initial_cash_cents: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
) -> BacktestBridgeResult {
    backtest_weights_with_options(
        weight_schedule,
        price_schedule,
        initial_cash_cents,
        cost_bps,
        periods_per_year,
        risk_free,
        BacktestBridgeOptions::default(),
    )
}

/// Simulate portfolio returns from a pre-computed weight schedule with optional v0.9 features.
///
/// Returns an empty result (no returns, no metrics) for invalid inputs:
/// mismatched schedule lengths, non-positive cash, NaN/Inf weights,
/// negative prices, or cost > 100%.
pub fn backtest_weights_with_options(
    weight_schedule: &[Vec<(Symbol, f64)>],
    price_schedule: &[Vec<(Symbol, i64)>],
    initial_cash_cents: i64,
    cost_bps: u32,
    periods_per_year: f64,
    risk_free: f64,
    options: BacktestBridgeOptions,
) -> BacktestBridgeResult {
    if !valid_inputs(
        weight_schedule,
        price_schedule,
        initial_cash_cents,
        cost_bps,
    ) {
        return empty_result(initial_cash_cents);
    }

    let stop_cfg = options
        .stop_cfg
        .as_ref()
        .and_then(BacktestStopConfig::sanitized);

    let cost_model = CostModel {
        commission_bps: cost_bps,
        slippage_bps: 0,
        min_trade_fee: 0,
    };

    let mut portfolio = Portfolio::new(initial_cash_cents, cost_model);
    let mut equity_curve = Vec::with_capacity(weight_schedule.len() + 1);
    equity_curve.push(initial_cash_cents);

    let mut holdings = Vec::with_capacity(weight_schedule.len());
    let mut symbol_returns = Vec::with_capacity(weight_schedule.len());
    let mut stop_events = Vec::new();

    let mut prev_prices: HashMap<Symbol, i64> = HashMap::new();
    let mut stop_trackers: HashMap<Symbol, StopTracker> = HashMap::new();

    for (period_index, (weights, prices)) in weight_schedule
        .iter()
        .zip(price_schedule.iter())
        .enumerate()
    {
        let price_map: HashMap<Symbol, i64> = prices.iter().copied().collect();

        let mut period_symbol_returns = Vec::with_capacity(prices.len());
        for &(sym, px) in prices {
            let ret = prev_prices
                .get(&sym)
                .copied()
                .and_then(|p0| {
                    if p0 > 0 && px > 0 {
                        Some((px - p0) as f64 / p0 as f64)
                    } else {
                        None
                    }
                })
                .unwrap_or(f64::NAN);
            period_symbol_returns.push((sym, ret));
        }
        period_symbol_returns.sort_by_key(|(sym, _)| *sym);
        symbol_returns.push(period_symbol_returns);

        // Rebalance to target weights first.
        portfolio.rebalance_simple(weights, prices);

        // Optional stop simulation runs after target rebalance on each bar.
        if let Some(cfg) = stop_cfg.as_ref() {
            apply_stop_cfg(
                &mut portfolio,
                &price_map,
                period_index,
                cfg,
                &mut stop_trackers,
                &mut stop_events,
            );
        }

        // Record return for this period.
        portfolio.record_return(prices);

        // Track holdings and equity.
        let mut period_holdings = portfolio.current_weights(prices);
        period_holdings.sort_by_key(|(sym, _)| *sym);
        holdings.push(period_holdings);

        let equity = portfolio.total_equity(prices);
        equity_curve.push(equity);

        prev_prices = price_map;
    }

    let returns = portfolio.returns().to_vec();
    let metrics = compute_metrics(&returns, periods_per_year, risk_free);

    BacktestBridgeResult {
        returns,
        equity_curve,
        final_cash: portfolio.cash(),
        metrics,
        holdings,
        symbol_returns,
        stop_events,
    }
}

/// Decompose a weight/return schedule into per-symbol contributions and trades.
///
/// Each period contribution is `weight * period_return` for that symbol. Cumulative
/// contribution is a simple running sum per symbol. Trade events are derived from
/// target-weight transitions: zero→non-zero opens a trade, non-zero→zero closes it.
pub fn tear_sheet(
    result: &BacktestBridgeResult,
    rolling_window: usize,
    periods_per_year: usize,
) -> TearSheet {
    let attribution = decompose_backtest(&result.holdings, &result.symbol_returns);
    let equity: Vec<f64> = result
        .equity_curve
        .iter()
        .map(|value| *value as f64)
        .collect();
    TearSheet {
        monthly_returns: monthly_return_matrix(&result.returns, 21),
        rolling_sharpe: rolling_sharpe(&result.returns, rolling_window, periods_per_year),
        drawdown_events: drawdown_series(&equity),
        trade_analytics: TradeAnalytics {
            trade_count: attribution.trades.len(),
            open_trade_count: attribution
                .trades
                .iter()
                .filter(|trade| trade.exit_index.is_none())
                .count(),
            closed_trade_count: attribution
                .trades
                .iter()
                .filter(|trade| trade.exit_index.is_some())
                .count(),
        },
    }
}

fn monthly_return_matrix(returns: &[f64], periods_per_month: usize) -> Vec<Vec<f64>> {
    if periods_per_month == 0 {
        return Vec::new();
    }
    returns
        .chunks(periods_per_month)
        .map(|chunk| chunk.iter().fold(1.0, |acc, value| acc * (1.0 + value)) - 1.0)
        .collect::<Vec<_>>()
        .chunks(12)
        .map(|year| year.to_vec())
        .collect()
}

pub fn decompose_backtest(
    weight_schedule: &[Vec<(Symbol, f64)>],
    return_schedule: &[Vec<(Symbol, f64)>],
) -> AttributionResult {
    if weight_schedule.len() != return_schedule.len() {
        return AttributionResult {
            contributions: Vec::new(),
            cumulative_contributions: Vec::new(),
            trades: Vec::new(),
        };
    }

    let mut cumulative: HashMap<Symbol, f64> = HashMap::new();
    let mut previous_weights: HashMap<Symbol, f64> = HashMap::new();
    let mut open_trades: HashMap<Symbol, (usize, f64)> = HashMap::new();
    let mut contributions = Vec::with_capacity(weight_schedule.len());
    let mut cumulative_contributions = Vec::with_capacity(weight_schedule.len());
    let mut trades = Vec::new();

    for (period_index, (weights, returns)) in
        weight_schedule.iter().zip(return_schedule).enumerate()
    {
        let weight_map: HashMap<Symbol, f64> = weights.iter().copied().collect();
        let return_map: HashMap<Symbol, f64> = returns.iter().copied().collect();
        let mut symbols: Vec<Symbol> = weight_map
            .keys()
            .chain(return_map.keys())
            .chain(previous_weights.keys())
            .copied()
            .collect();
        symbols.sort_unstable();
        symbols.dedup();

        let mut period_contrib = Vec::new();
        let mut period_cumulative = Vec::new();

        for symbol in symbols {
            let weight = weight_map
                .get(&symbol)
                .copied()
                .filter(|value| value.is_finite())
                .unwrap_or(0.0);
            let previous = previous_weights
                .get(&symbol)
                .copied()
                .filter(|value| value.is_finite())
                .unwrap_or(0.0);

            if previous == 0.0 && weight != 0.0 {
                open_trades.insert(symbol, (period_index, weight));
            } else if previous != 0.0 && weight == 0.0 {
                if let Some((entry_index, entry_weight)) = open_trades.remove(&symbol) {
                    trades.push(AttributionTrade {
                        symbol,
                        entry_index,
                        exit_index: Some(period_index),
                        entry_weight,
                        exit_weight: previous,
                    });
                }
            }

            let period_return = return_map
                .get(&symbol)
                .copied()
                .filter(|value| value.is_finite())
                .unwrap_or(0.0);
            let contribution = weight * period_return;
            if contribution != 0.0 || weight != 0.0 || previous != 0.0 {
                let running = cumulative.entry(symbol).or_insert(0.0);
                *running += contribution;
                period_contrib.push((symbol, contribution));
                period_cumulative.push((symbol, *running));
            }
        }

        period_contrib.sort_by_key(|(symbol, _)| *symbol);
        period_cumulative.sort_by_key(|(symbol, _)| *symbol);
        contributions.push(period_contrib);
        cumulative_contributions.push(period_cumulative);
        previous_weights = weight_map;
    }

    for (symbol, (entry_index, entry_weight)) in open_trades {
        let exit_weight = previous_weights
            .get(&symbol)
            .copied()
            .unwrap_or(entry_weight);
        trades.push(AttributionTrade {
            symbol,
            entry_index,
            exit_index: None,
            entry_weight,
            exit_weight,
        });
    }
    trades.sort_by_key(|trade| (trade.entry_index, trade.symbol));

    AttributionResult {
        contributions,
        cumulative_contributions,
        trades,
    }
}

fn valid_inputs(
    weight_schedule: &[Vec<(Symbol, f64)>],
    price_schedule: &[Vec<(Symbol, i64)>],
    initial_cash_cents: i64,
    cost_bps: u32,
) -> bool {
    if weight_schedule.len() != price_schedule.len() {
        return false;
    }
    if initial_cash_cents <= 0 {
        return false;
    }
    if cost_bps > 10_000 {
        return false;
    }

    for (weights, prices) in weight_schedule.iter().zip(price_schedule.iter()) {
        for &(_, w) in weights {
            if !w.is_finite() {
                return false;
            }
        }
        for &(_, p) in prices {
            if p < 0 {
                return false;
            }
        }
    }

    true
}

fn empty_result(initial_cash_cents: i64) -> BacktestBridgeResult {
    BacktestBridgeResult {
        returns: Vec::new(),
        equity_curve: vec![initial_cash_cents],
        final_cash: initial_cash_cents,
        metrics: None,
        holdings: Vec::new(),
        symbol_returns: Vec::new(),
        stop_events: Vec::new(),
    }
}

#[derive(Clone, Debug)]
struct StopTracker {
    side: i8, // +1 long, -1 short
    entry_price: i64,
    reference_price: i64,
    last_price: i64,
    abs_changes: Vec<i64>,
}

impl StopTracker {
    fn new(entry_price: i64, side: i8) -> Self {
        Self {
            side,
            entry_price,
            reference_price: entry_price,
            last_price: entry_price,
            abs_changes: Vec::new(),
        }
    }

    fn update(&mut self, price: i64, atr_period: usize) {
        if price <= 0 {
            return;
        }

        let delta = (price - self.last_price).abs();
        self.abs_changes.push(delta);
        let keep = atr_period.max(1) * 6;
        if self.abs_changes.len() > keep {
            let drop_n = self.abs_changes.len() - keep;
            self.abs_changes.drain(..drop_n);
        }

        self.last_price = price;
        if self.side > 0 {
            self.reference_price = self.reference_price.max(price);
        } else {
            self.reference_price = self.reference_price.min(price);
        }
    }

    fn atr(&self, atr_period: usize) -> Option<f64> {
        if self.abs_changes.is_empty() {
            return None;
        }

        let k = atr_period.max(1).min(self.abs_changes.len());
        // Safe: k is bounded by [1, abs_changes.len()], so len() - k >= 0
        let tail = &self.abs_changes[self.abs_changes.len() - k..];
        let mean = tail.iter().map(|x| *x as f64).sum::<f64>() / k as f64;
        Some(mean)
    }
}

fn apply_stop_cfg(
    portfolio: &mut Portfolio,
    price_map: &HashMap<Symbol, i64>,
    period_index: usize,
    cfg: &BacktestStopConfig,
    trackers: &mut HashMap<Symbol, StopTracker>,
    stop_events: &mut Vec<BacktestStopEvent>,
) {
    let open_positions: Vec<(Symbol, i64, i64)> = portfolio
        .positions()
        .filter_map(|(sym, pos)| {
            if pos.is_flat() {
                return None;
            }
            let px = price_map.get(sym).copied()?;
            if px <= 0 {
                return None;
            }
            Some((*sym, pos.quantity, px))
        })
        .collect();

    let open_symbols: HashSet<Symbol> = open_positions.iter().map(|(s, _, _)| *s).collect();
    trackers.retain(|sym, _| open_symbols.contains(sym));

    for (sym, qty, price) in open_positions {
        let side = if qty >= 0 { 1 } else { -1 };

        let tracker = trackers
            .entry(sym)
            .or_insert_with(|| StopTracker::new(price, side));

        if tracker.side != side {
            *tracker = StopTracker::new(price, side);
        } else {
            tracker.update(price, cfg.atr_period);
        }

        let Some((stop_level, reason)) = effective_stop_level(cfg, tracker) else {
            continue;
        };

        let breached = if side > 0 {
            price <= stop_level
        } else {
            price >= stop_level
        };

        if breached {
            let closed = portfolio.close_position_at(sym, price);
            if closed {
                stop_events.push(BacktestStopEvent {
                    period_index,
                    symbol: sym,
                    trigger_price: stop_level,
                    exit_price: price,
                    reason,
                });
                trackers.remove(&sym);
            }
        }
    }
}

fn effective_stop_level(
    cfg: &BacktestStopConfig,
    tracker: &StopTracker,
) -> Option<(i64, &'static str)> {
    let mut candidates = Vec::new();

    if let Some(p) = cfg.fixed_stop_pct {
        let level = if tracker.side > 0 {
            (tracker.entry_price as f64 * (1.0 - p)).round() as i64
        } else {
            (tracker.entry_price as f64 * (1.0 + p)).round() as i64
        }
        .max(1);
        candidates.push((level, "fixed"));
    }

    if let Some(p) = cfg.trailing_stop_pct {
        let level = if tracker.side > 0 {
            (tracker.reference_price as f64 * (1.0 - p)).round() as i64
        } else {
            (tracker.reference_price as f64 * (1.0 + p)).round() as i64
        }
        .max(1);
        candidates.push((level, "trailing"));
    }

    if let Some(mult) = cfg.atr_multiple
        && let Some(atr) = tracker.atr(cfg.atr_period)
    {
        let level = if tracker.side > 0 {
            (tracker.reference_price as f64 - mult * atr).round() as i64
        } else {
            (tracker.reference_price as f64 + mult * atr).round() as i64
        }
        .max(1);
        candidates.push((level, "atr"));
    }

    if candidates.is_empty() {
        return None;
    }

    if tracker.side > 0 {
        candidates.into_iter().max_by_key(|(level, _)| *level)
    } else {
        candidates.into_iter().min_by_key(|(level, _)| *level)
    }
}

fn sanitize_pct(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0 && *x < 1.0)
}

fn sanitize_positive(v: Option<f64>) -> Option<f64> {
    v.filter(|x| x.is_finite() && *x > 0.0)
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
    fn tear_sheet_contains_reporting_payload() {
        let weights = vec![vec![(aapl(), 1.0)]; 24];
        let prices: Vec<Vec<(Symbol, i64)>> = (0..24)
            .map(|i| vec![(aapl(), 100_00 + i as i64 * 100)])
            .collect();
        let result = backtest_weights(&weights, &prices, 1_000_000_00, 0, 252.0, 0.0);

        let sheet = tear_sheet(&result, 5, 252);

        assert_eq!(sheet.monthly_returns.len(), 1);
        assert_eq!(sheet.monthly_returns[0].len(), 2);
        assert_eq!(sheet.rolling_sharpe.len(), result.returns.len());
        assert!(sheet.trade_analytics.trade_count >= 1);
    }

    #[test]
    fn monthly_return_matrix_compounds_chunks() {
        let matrix = monthly_return_matrix(&[0.1, 0.1, -0.1], 2);
        assert_eq!(matrix.len(), 1);
        assert!((matrix[0][0] - 0.21).abs() < 1e-12);
        assert!((matrix[0][1] + 0.1).abs() < 1e-12);
    }

    #[test]
    fn decompose_backtest_computes_contribution_and_cumulative_sum() {
        let weights = vec![
            vec![(aapl(), 0.6), (msft(), 0.4)],
            vec![(aapl(), 0.5), (msft(), 0.5)],
        ];
        let returns = vec![
            vec![(aapl(), 0.10), (msft(), -0.05)],
            vec![(aapl(), 0.02), (msft(), 0.04)],
        ];

        let result = decompose_backtest(&weights, &returns);

        assert_eq!(
            result.contributions[0],
            vec![(aapl(), 0.06), (msft(), -0.020000000000000004)]
        );
        assert_eq!(
            result.contributions[1],
            vec![(aapl(), 0.01), (msft(), 0.02)]
        );
        assert!((result.cumulative_contributions[1][0].1 - 0.07).abs() < 1e-12);
        assert!((result.cumulative_contributions[1][1].1 - 0.0).abs() < 1e-12);
    }

    #[test]
    fn decompose_backtest_detects_entries_exits_and_open_trades() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 0.5), (msft(), 0.5)],
            vec![(msft(), 1.0)],
        ];
        let returns = vec![
            vec![(aapl(), 0.01)],
            vec![(aapl(), 0.01), (msft(), 0.02)],
            vec![(msft(), 0.03)],
        ];

        let result = decompose_backtest(&weights, &returns);

        assert!(result.trades.iter().any(|trade| {
            trade.symbol == aapl()
                && trade.entry_index == 0
                && trade.exit_index == Some(2)
                && trade.entry_weight == 1.0
                && trade.exit_weight == 0.5
        }));
        assert!(result.trades.iter().any(|trade| {
            trade.symbol == msft() && trade.entry_index == 1 && trade.exit_index.is_none()
        }));
    }

    #[test]
    fn decompose_backtest_rejects_mismatched_lengths_with_empty_result() {
        let result = decompose_backtest(&[vec![(aapl(), 1.0)]], &[]);
        assert!(result.contributions.is_empty());
        assert!(result.cumulative_contributions.is_empty());
        assert!(result.trades.is_empty());
    }

    #[test]
    fn decompose_backtest_integrates_with_backtest_weights_outputs() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 110_00)],
            vec![(aapl(), 99_00)],
        ];
        let backtest = backtest_weights(&weights, &prices, 1_000_000_00, 0, 252.0, 0.0);

        let attribution = decompose_backtest(&backtest.holdings, &backtest.symbol_returns);

        for (period, contributions) in attribution.contributions.iter().enumerate() {
            let summed: f64 = contributions.iter().map(|(_, value)| value).sum();
            assert!((summed - backtest.returns[period]).abs() < 1e-12);
        }
    }

    #[test]
    fn decompose_backtest_treats_non_finite_returns_as_zero() {
        let weights = vec![vec![(aapl(), 1.0)]];
        let returns = vec![vec![(aapl(), f64::NAN)]];

        let result = decompose_backtest(&weights, &returns);

        assert_eq!(result.contributions, vec![vec![(aapl(), 0.0)]]);
        assert_eq!(result.cumulative_contributions, vec![vec![(aapl(), 0.0)]]);
    }

    #[test]
    fn basic_two_period_backtest() {
        let weights = vec![
            vec![(aapl(), 0.5), (msft(), 0.5)],
            vec![(aapl(), 0.3), (msft(), 0.7)],
        ];
        let prices = vec![
            vec![(aapl(), 150_00), (msft(), 300_00)],
            vec![(aapl(), 155_00), (msft(), 310_00)],
        ];

        let result = backtest_weights(&weights, &prices, 1_000_000_00, 10, 252.0, 0.0);

        assert_eq!(result.returns.len(), 2);
        assert_eq!(result.equity_curve.len(), 3); // initial + 2 periods
        assert!(result.metrics.is_some());
        assert_eq!(result.holdings.len(), 2);
        assert_eq!(result.symbol_returns.len(), 2);
    }

    #[test]
    fn zero_cost_preserves_equity() {
        let weights = vec![vec![(aapl(), 0.5)]];
        let prices = vec![vec![(aapl(), 100_00)]];

        let result = backtest_weights(&weights, &prices, 1_000_000_00, 0, 252.0, 0.0);

        // With zero cost and no price movement, equity should be ~initial
        let final_eq = *result
            .equity_curve
            .last()
            .expect("equity curve has one point");
        assert!((final_eq - 1_000_000_00).abs() < 200_00); // rounding tolerance
    }

    #[test]
    fn empty_schedule() {
        let result = backtest_weights(&[], &[], 1_000_000_00, 10, 252.0, 0.0);
        assert!(result.returns.is_empty());
        assert!(result.metrics.is_none());
        assert_eq!(result.equity_curve.len(), 1);
        assert!(result.holdings.is_empty());
        assert!(result.symbol_returns.is_empty());
    }

    #[test]
    fn fixed_stop_triggers_exit() {
        let weights = vec![vec![(aapl(), 1.0)], vec![(aapl(), 1.0)]];
        let prices = vec![vec![(aapl(), 100_00)], vec![(aapl(), 85_00)]];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].reason, "fixed");
        assert_eq!(result.stop_events[0].period_index, 1);
        assert_eq!(result.stop_events[0].trigger_price, 90_00);
        assert_eq!(result.stop_events[0].exit_price, 85_00);
        assert!(result.holdings[1].is_empty());
    }

    #[test]
    fn trailing_stop_emits_event() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 110_00)],
            vec![(aapl(), 95_00)],
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: None,
                trailing_stop_pct: Some(0.10),
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert!(!result.stop_events.is_empty());
        assert_eq!(result.stop_events[0].reason, "trailing");
    }

    #[test]
    fn first_breach_triggers_once_per_position_lifecycle() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 90_00)], // fixed 10% stop breaches here
            vec![(aapl(), 89_00)], // reopened, new stop basis, no second trigger
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].period_index, 1);
        assert_eq!(result.stop_events[0].reason, "fixed");
    }

    #[test]
    fn tighter_stop_reason_is_reported_when_multiple_rules_enabled() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 110_00)], // updates trailing reference
            vec![(aapl(), 103_00)], // breaches trailing(104.5) but not fixed(90)
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: Some(0.05),
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].reason, "trailing");
        assert_eq!(result.stop_events[0].trigger_price, 104_50);
    }

    #[test]
    fn atr_stop_triggers_on_high_volatility() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        // High volatility: 100 -> 110 -> 95 -> 85 (large moves)
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 110_00)],
            vec![(aapl(), 95_00)],
            vec![(aapl(), 85_00)],
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: None,
                trailing_stop_pct: None,
                atr_multiple: Some(2.0), // 2x ATR stop
                atr_period: 3,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        // Should trigger on high volatility
        assert!(!result.stop_events.is_empty());
        assert_eq!(result.stop_events[0].reason, "atr");
    }

    #[test]
    fn short_position_fixed_stop_triggers_on_rise() {
        let weights = vec![vec![(aapl(), -1.0)], vec![(aapl(), -1.0)]];
        // Short position: stop triggers when price rises
        let prices = vec![vec![(aapl(), 100_00)], vec![(aapl(), 115_00)]];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10), // 10% stop
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].reason, "fixed");
        assert_eq!(result.stop_events[0].trigger_price, 110_00); // 100 * 1.10
        assert_eq!(result.stop_events[0].exit_price, 115_00);
    }

    #[test]
    fn short_position_trailing_stop_adjusts_downward() {
        let weights = vec![
            vec![(aapl(), -1.0)],
            vec![(aapl(), -1.0)],
            vec![(aapl(), -1.0)],
        ];
        // Short: trailing stop moves down as price falls (protects profit)
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 90_00)], // profit, trailing stop adjusts down
            vec![(aapl(), 98_00)], // rises but doesn't hit adjusted stop
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: None,
                trailing_stop_pct: Some(0.05),
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        // Should not trigger - price rose but trailing stop adjusted down
        assert!(result.stop_events.is_empty());
    }

    #[test]
    fn multiple_symbols_independent_stops() {
        let weights = vec![
            vec![(aapl(), 0.5), (msft(), 0.5)],
            vec![(aapl(), 0.5), (msft(), 0.5)],
        ];
        // AAPL drops 15% (triggers 10% stop), MSFT drops 5% (no trigger)
        let prices = vec![
            vec![(aapl(), 100_00), (msft(), 100_00)],
            vec![(aapl(), 85_00), (msft(), 95_00)],
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].symbol, aapl());
        assert!(result.holdings[1].iter().all(|(sym, _)| *sym != aapl()));
    }

    #[test]
    fn position_flip_resets_stop_tracker() {
        let weights = vec![
            vec![(aapl(), 1.0)],  // long
            vec![(aapl(), -1.0)], // flip to short
            vec![(aapl(), -1.0)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 95_00)],
            vec![(aapl(), 110_00)], // short stop would trigger at 105
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        // Should trigger on short position after flip
        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].period_index, 2);
    }

    #[test]
    fn stop_loss_with_low_volatility_no_atr_trigger() {
        let weights = vec![
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
            vec![(aapl(), 1.0)],
        ];
        // Low volatility: small price moves
        let prices = vec![
            vec![(aapl(), 100_00)],
            vec![(aapl(), 101_00)],
            vec![(aapl(), 102_00)],
            vec![(aapl(), 101_50)],
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: None,
                trailing_stop_pct: None,
                atr_multiple: Some(3.0), // high multiple but low volatility
                atr_period: 3,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        // Should not trigger - low volatility keeps ATR small
        assert!(result.stop_events.is_empty());
    }

    #[test]
    fn stop_loss_with_rebalance_keeps_tracking() {
        let weights = vec![
            vec![(aapl(), 0.8), (msft(), 0.2)],
            vec![(aapl(), 0.6), (msft(), 0.4)], // rebalance
            vec![(aapl(), 0.6), (msft(), 0.4)],
        ];
        let prices = vec![
            vec![(aapl(), 100_00), (msft(), 100_00)],
            vec![(aapl(), 95_00), (msft(), 95_00)], // both drop
            vec![(aapl(), 85_00), (msft(), 95_00)], // AAPL triggers
        ];

        let options = BacktestBridgeOptions {
            stop_cfg: Some(BacktestStopConfig {
                fixed_stop_pct: Some(0.10),
                trailing_stop_pct: None,
                atr_multiple: None,
                atr_period: 14,
            }),
        };

        let result =
            backtest_weights_with_options(&weights, &prices, 100_000_00, 0, 252.0, 0.0, options);

        // AAPL should trigger, MSFT should continue
        assert_eq!(result.stop_events.len(), 1);
        assert_eq!(result.stop_events[0].symbol, aapl());
        assert!(result.holdings[2].iter().any(|(sym, _)| *sym == msft()));
    }
}
