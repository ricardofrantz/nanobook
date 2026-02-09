//! Risk configuration.

/// Configuration for the risk engine.
#[derive(Debug, Clone)]
pub struct RiskConfig {
    /// Max single-position weight as fraction (e.g., 0.20 = 20%).
    pub max_position_pct: f64,
    /// Max order value in USD.
    pub max_order_value_cents: i64,
    /// Max batch (rebalance) value in USD cents.
    pub max_batch_value_cents: i64,
    /// Max gross leverage (1.0 for long-only).
    pub max_leverage: f64,
    /// Max drawdown fraction (circuit breaker).
    pub max_drawdown_pct: f64,
    /// Allow short selling.
    pub allow_short: bool,
    /// Max short exposure as fraction of equity.
    pub max_short_pct: f64,
    /// Min trade size in USD.
    pub min_trade_usd: f64,
    /// Max trade size in USD.
    pub max_trade_usd: f64,
}

impl RiskConfig {
    /// Validate the config. Returns `Err` with a description if any field is nonsensical.
    pub fn validate(&self) -> Result<(), String> {
        if !self.max_position_pct.is_finite()
            || self.max_position_pct <= 0.0
            || self.max_position_pct > 1.0
        {
            return Err(format!(
                "max_position_pct must be in (0, 1], got {}",
                self.max_position_pct
            ));
        }
        if !self.max_leverage.is_finite() || self.max_leverage < 1.0 {
            return Err(format!(
                "max_leverage must be >= 1.0 and finite, got {}",
                self.max_leverage
            ));
        }
        if !self.max_drawdown_pct.is_finite()
            || self.max_drawdown_pct < 0.0
            || self.max_drawdown_pct > 1.0
        {
            return Err(format!(
                "max_drawdown_pct must be in [0, 1], got {}",
                self.max_drawdown_pct
            ));
        }
        if !self.max_short_pct.is_finite() || self.max_short_pct < 0.0 {
            return Err(format!(
                "max_short_pct must be >= 0 and finite, got {}",
                self.max_short_pct
            ));
        }
        if !self.min_trade_usd.is_finite() || self.min_trade_usd < 0.0 {
            return Err(format!(
                "min_trade_usd must be >= 0 and finite, got {}",
                self.min_trade_usd
            ));
        }
        if !self.max_trade_usd.is_finite() || self.max_trade_usd < 0.0 {
            return Err(format!(
                "max_trade_usd must be >= 0 and finite, got {}",
                self.max_trade_usd
            ));
        }
        Ok(())
    }
}

impl Default for RiskConfig {
    fn default() -> Self {
        Self {
            max_position_pct: 0.25,
            max_order_value_cents: 10_000_000,  // $100K
            max_batch_value_cents: 100_000_000, // $1M
            max_leverage: 1.5,
            max_drawdown_pct: 0.20,
            allow_short: true,
            max_short_pct: 0.30,
            min_trade_usd: 100.0,
            max_trade_usd: 100_000.0,
        }
    }
}
