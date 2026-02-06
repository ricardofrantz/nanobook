//! Financial performance metrics.

/// Computed performance metrics for a return series.
///
/// All return-based metrics assume simple (not log) returns.
/// Annualization uses the `periods_per_year` parameter
/// (e.g., 252 for daily, 12 for monthly, 52 for weekly).
#[derive(Clone, Debug)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct Metrics {
    /// Total cumulative return (e.g., 0.15 = 15%)
    pub total_return: f64,
    /// Compound annual growth rate
    pub cagr: f64,
    /// Annualized volatility (standard deviation of returns)
    pub volatility: f64,
    /// Annualized Sharpe ratio: (mean return - risk_free) / volatility
    pub sharpe: f64,
    /// Annualized Sortino ratio: (mean return - risk_free) / downside_deviation
    pub sortino: f64,
    /// Maximum drawdown (as positive fraction, e.g., 0.20 = 20% peak-to-trough)
    pub max_drawdown: f64,
    /// Calmar ratio: CAGR / max_drawdown
    pub calmar: f64,
    /// Number of return periods
    pub num_periods: usize,
    /// Periods with positive return
    pub winning_periods: usize,
    /// Periods with negative return
    pub losing_periods: usize,
}

impl std::fmt::Display for Metrics {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Performance Metrics")?;
        writeln!(f, "  Total return:    {:>8.2}%", self.total_return * 100.0)?;
        writeln!(f, "  CAGR:            {:>8.2}%", self.cagr * 100.0)?;
        writeln!(f, "  Volatility:      {:>8.2}%", self.volatility * 100.0)?;
        writeln!(f, "  Sharpe:          {:>8.2}", self.sharpe)?;
        writeln!(f, "  Sortino:         {:>8.2}", self.sortino)?;
        writeln!(f, "  Max drawdown:    {:>8.2}%", self.max_drawdown * 100.0)?;
        writeln!(f, "  Calmar:          {:>8.2}", self.calmar)?;
        writeln!(
            f,
            "  Win/Loss/Total:  {}/{}/{}",
            self.winning_periods, self.losing_periods, self.num_periods
        )
    }
}

/// Compute performance metrics from a series of periodic returns.
///
/// # Arguments
///
/// * `returns` — Slice of simple returns (e.g., `[0.01, -0.005, 0.02]`)
/// * `periods_per_year` — Annualization factor (252 for daily, 12 for monthly)
/// * `risk_free` — Risk-free rate per period (e.g., 0.04/252 for 4% annual)
///
/// Returns `None` if `returns` is empty.
pub fn compute_metrics(returns: &[f64], periods_per_year: f64, risk_free: f64) -> Option<Metrics> {
    if returns.is_empty() {
        return None;
    }

    let n = returns.len();

    // Total return: product of (1 + r_i) - 1
    let total_return = returns.iter().fold(1.0_f64, |acc, &r| acc * (1.0 + r)) - 1.0;

    // CAGR: (1 + total_return)^(periods_per_year / n) - 1
    let years = n as f64 / periods_per_year;
    let cagr = if years > 0.0 && total_return > -1.0 {
        (1.0 + total_return).powf(1.0 / years) - 1.0
    } else if total_return <= -1.0 {
        -1.0 // total or leveraged loss — clamp to -100%
    } else {
        0.0
    };

    // Mean return
    let mean = returns.iter().sum::<f64>() / n as f64;

    // Volatility (sample std dev, annualized)
    let variance = if n > 1 {
        returns.iter().map(|&r| (r - mean).powi(2)).sum::<f64>() / (n - 1) as f64
    } else {
        0.0
    };
    let volatility = variance.sqrt() * periods_per_year.sqrt();

    // Excess returns for Sharpe/Sortino
    let excess_mean = mean - risk_free;

    // Sharpe ratio (annualized)
    let sharpe = if volatility > 0.0 {
        excess_mean * periods_per_year.sqrt() / (variance.sqrt())
    } else {
        0.0
    };

    // Downside deviation (only negative excess returns)
    let downside_variance = if n > 1 {
        returns
            .iter()
            .map(|&r| {
                let excess = r - risk_free;
                if excess < 0.0 { excess.powi(2) } else { 0.0 }
            })
            .sum::<f64>()
            / (n - 1) as f64
    } else {
        0.0
    };
    let downside_dev = downside_variance.sqrt();

    // Sortino ratio (annualized)
    let sortino = if downside_dev > 0.0 {
        excess_mean * periods_per_year.sqrt() / downside_dev
    } else {
        0.0
    };

    // Max drawdown
    let max_drawdown = compute_max_drawdown(returns);

    // Calmar ratio
    let calmar = if max_drawdown > 0.0 {
        cagr / max_drawdown
    } else {
        0.0
    };

    // Win/loss counts
    let winning_periods = returns.iter().filter(|&&r| r > 0.0).count();
    let losing_periods = returns.iter().filter(|&&r| r < 0.0).count();

    Some(Metrics {
        total_return,
        cagr,
        volatility,
        sharpe,
        sortino,
        max_drawdown,
        calmar,
        num_periods: n,
        winning_periods,
        losing_periods,
    })
}

/// Compute maximum drawdown from a return series.
fn compute_max_drawdown(returns: &[f64]) -> f64 {
    let mut peak = 1.0_f64;
    let mut equity = 1.0_f64;
    let mut max_dd = 0.0_f64;

    for &r in returns {
        equity *= 1.0 + r;
        if equity > peak {
            peak = equity;
        }
        let dd = (peak - equity) / peak;
        if dd > max_dd {
            max_dd = dd;
        }
    }

    max_dd
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_returns() {
        assert!(compute_metrics(&[], 252.0, 0.0).is_none());
    }

    #[test]
    fn single_return() {
        let m = compute_metrics(&[0.05], 252.0, 0.0).unwrap();
        assert!((m.total_return - 0.05).abs() < 1e-10);
        assert_eq!(m.num_periods, 1);
        assert_eq!(m.winning_periods, 1);
        assert_eq!(m.losing_periods, 0);
    }

    #[test]
    fn constant_returns() {
        // 12 months of 1% return
        let returns = vec![0.01; 12];
        let m = compute_metrics(&returns, 12.0, 0.0).unwrap();

        // Total return: (1.01)^12 - 1 ≈ 12.68%
        assert!((m.total_return - 0.12682503).abs() < 1e-4);

        // CAGR should equal ~12.68% (exactly 1 year)
        assert!((m.cagr - m.total_return).abs() < 1e-6);

        // All winning
        assert_eq!(m.winning_periods, 12);
        assert_eq!(m.losing_periods, 0);
    }

    #[test]
    fn max_drawdown_simple() {
        // Up 10%, down 20%, up 5%
        let returns = vec![0.10, -0.20, 0.05];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();

        // Equity: 1.0 -> 1.1 -> 0.88 -> 0.924
        // Peak at 1.1, trough at 0.88, dd = (1.1 - 0.88) / 1.1 = 0.2
        assert!((m.max_drawdown - 0.2).abs() < 1e-10);
    }

    #[test]
    fn no_drawdown_when_always_up() {
        let returns = vec![0.01, 0.02, 0.03];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!((m.max_drawdown).abs() < 1e-10);
    }

    #[test]
    fn sharpe_positive_for_positive_returns() {
        let returns = vec![0.01, 0.02, 0.015, 0.005, 0.01];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!(m.sharpe > 0.0);
    }

    #[test]
    fn sortino_ge_sharpe_with_few_down_periods() {
        // Mostly positive returns → downside dev < total vol → Sortino > Sharpe
        let returns = vec![0.02, 0.03, 0.01, -0.005, 0.015];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!(m.sortino >= m.sharpe);
    }

    #[test]
    fn win_loss_count() {
        let returns = vec![0.01, -0.02, 0.0, 0.03, -0.01];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert_eq!(m.winning_periods, 2);
        assert_eq!(m.losing_periods, 2);
        assert_eq!(m.num_periods, 5);
    }

    #[test]
    fn calmar_ratio() {
        let returns = vec![0.01, -0.05, 0.02, 0.03, 0.01];
        let m = compute_metrics(&returns, 12.0, 0.0).unwrap();
        if m.max_drawdown > 0.0 && m.cagr != 0.0 {
            assert!((m.calmar - m.cagr / m.max_drawdown).abs() < 1e-10);
        }
    }

    #[test]
    fn display_format() {
        let returns = vec![0.01, -0.005, 0.02];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        let s = format!("{m}");
        assert!(s.contains("Total return:"));
        assert!(s.contains("Sharpe:"));
        assert!(s.contains("Max drawdown:"));
    }
}
