//! Target portfolio specification (target.json) loading and validation.

use std::path::Path;

use chrono::{DateTime, Utc};
use serde::Deserialize;

use crate::error::{Error, Result};

/// A target portfolio specification from the optimizer/bot.
#[derive(Debug, Clone, Deserialize)]
pub struct TargetSpec {
    pub timestamp: DateTime<Utc>,
    pub targets: Vec<TargetPosition>,
    #[serde(default)]
    pub constraints: Option<Constraints>,
}

/// A single target position: symbol + weight.
#[derive(Debug, Clone, Deserialize)]
pub struct TargetPosition {
    pub symbol: String,
    pub weight: f64,
}

/// Optional per-run constraint overrides.
#[derive(Debug, Clone, Deserialize)]
pub struct Constraints {
    pub max_position_pct: Option<f64>,
    pub max_leverage: Option<f64>,
    pub min_trade_usd: Option<f64>,
}

impl TargetSpec {
    /// Load and validate a target.json file.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::TargetRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let spec: TargetSpec = serde_json::from_str(&contents)?;
        spec.validate()?;
        Ok(spec)
    }

    /// Parse from a JSON string (useful for testing).
    pub fn from_json(json: &str) -> Result<Self> {
        let spec: TargetSpec = serde_json::from_str(json)?;
        spec.validate()?;
        Ok(spec)
    }

    /// Validate the target specification.
    fn validate(&self) -> Result<()> {
        if self.targets.is_empty() {
            return Err(Error::Target("targets list is empty".into()));
        }

        // Check for duplicate symbols
        let mut seen = std::collections::HashSet::new();
        for t in &self.targets {
            if !seen.insert(&t.symbol) {
                return Err(Error::Target(format!("duplicate symbol: {}", t.symbol)));
            }
        }

        // Validate each symbol fits in nanobook's Symbol (max 8 bytes)
        for t in &self.targets {
            if t.symbol.is_empty() {
                return Err(Error::Target("empty symbol".into()));
            }
            if t.symbol.len() > 8 {
                return Err(Error::Target(format!(
                    "symbol '{}' exceeds 8 bytes",
                    t.symbol
                )));
            }
        }

        // Validate weight magnitudes
        for t in &self.targets {
            if t.weight.abs() > 1.0 {
                return Err(Error::Target(format!(
                    "weight for {} ({}) has magnitude > 1.0",
                    t.symbol, t.weight
                )));
            }
            if t.weight == 0.0 {
                return Err(Error::Target(format!(
                    "weight for {} is zero — omit instead",
                    t.symbol
                )));
            }
        }

        // Sum of absolute weights should be reasonable (allow up to max_leverage)
        let long_sum: f64 = self
            .targets
            .iter()
            .filter(|t| t.weight > 0.0)
            .map(|t| t.weight)
            .sum();
        if long_sum > 1.0 {
            return Err(Error::Target(format!(
                "long weights sum to {long_sum:.4} (> 1.0)"
            )));
        }

        Ok(())
    }

    /// Get the list of symbols as nanobook `Symbol` values.
    pub fn symbols(&self) -> Vec<nanobook::Symbol> {
        self.targets
            .iter()
            .map(|t| nanobook::Symbol::new(&t.symbol))
            .collect()
    }

    /// Get (Symbol, weight) pairs for the diff engine.
    pub fn as_target_pairs(&self) -> Vec<(nanobook::Symbol, f64)> {
        self.targets
            .iter()
            .map(|t| (nanobook::Symbol::new(&t.symbol), t.weight))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_json() -> &'static str {
        r#"{
            "timestamp": "2026-02-08T15:30:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.40 },
                { "symbol": "MSFT", "weight": 0.30 },
                { "symbol": "SPY",  "weight": -0.10 },
                { "symbol": "QQQ",  "weight": 0.20 }
            ]
        }"#
    }

    #[test]
    fn parse_valid_target() {
        let spec = TargetSpec::from_json(valid_json()).unwrap();
        assert_eq!(spec.targets.len(), 4);
        assert_eq!(spec.targets[0].symbol, "AAPL");
        assert_eq!(spec.targets[0].weight, 0.40);
        assert_eq!(spec.targets[2].weight, -0.10); // short
    }

    #[test]
    fn symbols_conversion() {
        let spec = TargetSpec::from_json(valid_json()).unwrap();
        let syms = spec.symbols();
        assert_eq!(syms.len(), 4);
        assert_eq!(syms[0].as_str(), "AAPL");
    }

    #[test]
    fn as_target_pairs() {
        let spec = TargetSpec::from_json(valid_json()).unwrap();
        let pairs = spec.as_target_pairs();
        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].1, 0.40);
    }

    #[test]
    fn reject_empty_targets() {
        let json = r#"{"timestamp":"2026-01-01T00:00:00Z","targets":[]}"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn reject_duplicate_symbols() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.5 },
                { "symbol": "AAPL", "weight": 0.3 }
            ]
        }"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn reject_long_symbol() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "TOOLONGNAME", "weight": 0.5 }
            ]
        }"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn reject_weight_over_one() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 1.5 }
            ]
        }"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn reject_zero_weight() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.0 }
            ]
        }"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn reject_long_sum_over_one() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.6 },
                { "symbol": "MSFT", "weight": 0.5 }
            ]
        }"#;
        assert!(TargetSpec::from_json(json).is_err());
    }

    #[test]
    fn accept_with_constraints() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.5 }
            ],
            "constraints": {
                "max_position_pct": 0.5,
                "max_leverage": 2.0,
                "min_trade_usd": 50.0
            }
        }"#;
        let spec = TargetSpec::from_json(json).unwrap();
        let c = spec.constraints.unwrap();
        assert_eq!(c.max_position_pct, Some(0.5));
        assert_eq!(c.max_leverage, Some(2.0));
    }

    #[test]
    fn accept_short_positions() {
        let json = r#"{
            "timestamp": "2026-01-01T00:00:00Z",
            "targets": [
                { "symbol": "AAPL", "weight": 0.60 },
                { "symbol": "SPY",  "weight": -0.20 }
            ]
        }"#;
        let spec = TargetSpec::from_json(json).unwrap();
        assert_eq!(spec.targets[1].weight, -0.20);
    }
}
