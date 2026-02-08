//! Post-execution reconciliation: compare actual positions vs target.

use nanobook::Symbol;
use rustc_hash::FxHashMap;
use serde::Serialize;

use crate::diff::CurrentPosition;

/// Reconciliation report comparing actual vs target.
#[derive(Debug, Clone, Serialize)]
pub struct ReconcileReport {
    pub entries: Vec<ReconcileEntry>,
    pub tracking_error_pct: f64,
}

/// One symbol's reconciliation entry.
#[derive(Debug, Clone, Serialize)]
pub struct ReconcileEntry {
    pub symbol: String,
    pub target_weight: f64,
    pub actual_weight: f64,
    pub diff_weight: f64,
    pub target_shares: i64,
    pub actual_shares: i64,
    pub diff_shares: i64,
}

/// Compare actual positions against targets.
///
/// Returns a report with per-symbol comparison and overall tracking error.
pub fn reconcile(
    actual_positions: &[CurrentPosition],
    targets: &[(Symbol, f64)],
    prices: &[(Symbol, i64)],
    equity_cents: i64,
) -> ReconcileReport {
    let price_map: FxHashMap<Symbol, i64> = prices.iter().copied().collect();
    let target_map: FxHashMap<Symbol, f64> = targets.iter().copied().collect();
    let actual_map: FxHashMap<Symbol, i64> = actual_positions
        .iter()
        .map(|p| (p.symbol, p.quantity))
        .collect();

    // Collect all symbols from both target and actual
    let mut all_symbols: Vec<Symbol> = target_map.keys().copied().collect();
    for p in actual_positions {
        if !target_map.contains_key(&p.symbol) {
            all_symbols.push(p.symbol);
        }
    }
    all_symbols.sort();
    all_symbols.dedup();

    let mut entries = Vec::new();
    let mut sum_sq_diff = 0.0_f64;

    for sym in &all_symbols {
        let price = price_map.get(sym).copied().unwrap_or(0);
        let target_weight = target_map.get(sym).copied().unwrap_or(0.0);
        let actual_qty = actual_map.get(sym).copied().unwrap_or(0);

        let actual_weight = if equity_cents > 0 && price > 0 {
            (actual_qty * price) as f64 / equity_cents as f64
        } else {
            0.0
        };

        let target_shares = if price > 0 {
            (equity_cents as f64 * target_weight / price as f64) as i64
        } else {
            0
        };

        let diff_weight = actual_weight - target_weight;
        sum_sq_diff += diff_weight * diff_weight;

        entries.push(ReconcileEntry {
            symbol: sym.as_str().to_string(),
            target_weight,
            actual_weight,
            diff_weight,
            target_shares,
            actual_shares: actual_qty,
            diff_shares: actual_qty - target_shares,
        });
    }

    let tracking_error_pct = (sum_sq_diff / all_symbols.len().max(1) as f64).sqrt() * 100.0;

    ReconcileReport {
        entries,
        tracking_error_pct,
    }
}

impl std::fmt::Display for ReconcileReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "RECONCILIATION:")?;
        writeln!(
            f,
            "  {:8} {:>10} {:>10} {:>10} {:>10} {:>10}",
            "Symbol", "Target%", "Actual%", "Diff%", "TargetQty", "ActualQty"
        )?;
        for e in &self.entries {
            writeln!(
                f,
                "  {:8} {:>9.2}% {:>9.2}% {:>+9.2}% {:>10} {:>10}",
                e.symbol,
                e.target_weight * 100.0,
                e.actual_weight * 100.0,
                e.diff_weight * 100.0,
                e.target_shares,
                e.actual_shares,
            )?;
        }
        writeln!(f, "\n  Tracking error: {:.3}%", self.tracking_error_pct)?;
        Ok(())
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

    #[test]
    fn perfect_match() {
        let positions = vec![CurrentPosition {
            symbol: aapl(),
            quantity: 2702,
            avg_cost_cents: 185_00,
        }];

        let targets = vec![(aapl(), 0.5)];
        let prices = vec![(aapl(), 185_00)];
        let equity = 1_000_000_00;

        let report = reconcile(&positions, &targets, &prices, equity);
        assert!(report.tracking_error_pct < 1.0);
    }

    #[test]
    fn missing_position() {
        let positions = vec![]; // no positions
        let targets = vec![(aapl(), 0.5)];
        let prices = vec![(aapl(), 185_00)];
        let equity = 1_000_000_00;

        let report = reconcile(&positions, &targets, &prices, equity);
        assert!(report.tracking_error_pct > 1.0); // significant error
        assert_eq!(report.entries[0].actual_shares, 0);
    }

    #[test]
    fn extra_position() {
        let positions = vec![
            CurrentPosition {
                symbol: aapl(),
                quantity: 2702,
                avg_cost_cents: 185_00,
            },
            CurrentPosition {
                symbol: msft(),
                quantity: 100, // not in targets
                avg_cost_cents: 400_00,
            },
        ];

        let targets = vec![(aapl(), 0.5)];
        let prices = vec![(aapl(), 185_00), (msft(), 410_00)];
        let equity = 1_000_000_00;

        let report = reconcile(&positions, &targets, &prices, equity);
        // MSFT should show up with target_weight=0 but actual > 0
        let msft_entry = report.entries.iter().find(|e| e.symbol == "MSFT").unwrap();
        assert_eq!(msft_entry.target_weight, 0.0);
        assert!(msft_entry.actual_shares > 0);
    }

    #[test]
    fn display_format() {
        let report = ReconcileReport {
            entries: vec![ReconcileEntry {
                symbol: "AAPL".into(),
                target_weight: 0.5,
                actual_weight: 0.49,
                diff_weight: -0.01,
                target_shares: 2702,
                actual_shares: 2648,
                diff_shares: -54,
            }],
            tracking_error_pct: 1.0,
        };
        let s = format!("{report}");
        assert!(s.contains("AAPL"));
        assert!(s.contains("Tracking error"));
    }
}
