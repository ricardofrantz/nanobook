//! Fast backtest bridge: simulate portfolio returns from a pre-computed weight schedule.
//!
//! Python computes the weight schedule (factor models, signals, etc.),
//! Rust handles the inner simulation loop (rebalance, track positions, compute returns).

use std::collections::{HashMap, HashSet};

use crate::portfolio::metrics::{Metrics, compute_metrics};
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

/// Result of a backtest simulation.
#[derive(Clone, Debug)]
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
}
