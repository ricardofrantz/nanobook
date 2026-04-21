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

    // --- v0.8 extended metrics ---
    /// Conditional Value at Risk at 95% confidence (mean of worst 5% returns)
    pub cvar_95: f64,
    /// Win rate: fraction of positive-return periods
    pub win_rate: f64,
    /// Profit factor: sum(positive returns) / |sum(negative returns)|
    pub profit_factor: f64,
    /// Payoff ratio: mean(winning returns) / |mean(losing returns)|
    pub payoff_ratio: f64,
    /// Kelly criterion: win_rate - (1 - win_rate) / payoff_ratio
    pub kelly: f64,
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
        )?;
        writeln!(f, "  CVaR (95%):      {:>8.2}%", self.cvar_95 * 100.0)?;
        writeln!(f, "  Win rate:        {:>8.2}%", self.win_rate * 100.0)?;
        writeln!(f, "  Profit factor:   {:>8.2}", self.profit_factor)?;
        writeln!(f, "  Payoff ratio:    {:>8.2}", self.payoff_ratio)?;
        write!(f, "  Kelly:           {:>8.2}%", self.kelly * 100.0)
    }
}

/// Compute performance metrics from a series of periodic returns.
///
/// # Arguments
///
/// * `returns` — Slice of simple returns (e.g., `[0.01, -0.005, 0.02]`)
/// * `periods_per_year` — Annualization factor (252 for daily, 12 for monthly).
///   Must be strictly positive and finite; otherwise `None` is returned.
/// * `risk_free` — Risk-free rate per period (e.g., 0.04/252 for 4% annual)
///
/// Returns `None` if `returns` is empty or `periods_per_year` is not
/// strictly positive and finite. The guard prevents silent `NaN`
/// Sharpe/Sortino values from `periods_per_year.sqrt()` on
/// non-positive or infinite inputs.
pub fn compute_metrics(returns: &[f64], periods_per_year: f64, risk_free: f64) -> Option<Metrics> {
    if returns.is_empty() {
        return None;
    }
    if !periods_per_year.is_finite() || periods_per_year <= 0.0 {
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

    // Sortino ratio (annualized). v0.10 default is ddof=0, matching
    // `quantstats.stats.sortino` and the standard practitioner
    // convention. v0.9 used ddof=1 (Bessel correction); callers who
    // need that value can use `sortino(returns, rf, periods, 1)`.
    let sortino = sortino(returns, risk_free, periods_per_year, 0);

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

    // --- v0.8 extended metrics ---

    // CVaR (95%): mean of the worst 5% of returns. v0.10 default is
    // historical (pure empirical); v0.9 used the parametric-normal
    // hybrid. Users who need the parametric variant can call
    // `cvar(returns, 0.05, CVaRMethod::ParametricNormal)` directly.
    let cvar_95 = cvar(returns, 0.05, CVaRMethod::Historical);

    // Win rate
    let win_rate = winning_periods as f64 / n as f64;

    // Profit factor: sum(positive) / |sum(negative)|
    let sum_positive: f64 = returns.iter().filter(|&&r| r > 0.0).sum();
    let sum_negative: f64 = returns.iter().filter(|&&r| r < 0.0).sum();
    let profit_factor = if sum_negative != 0.0 {
        sum_positive / sum_negative.abs()
    } else if sum_positive > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    // Payoff ratio: mean(winning) / |mean(losing)|
    let mean_winning = if winning_periods > 0 {
        sum_positive / winning_periods as f64
    } else {
        0.0
    };
    let mean_losing = if losing_periods > 0 {
        sum_negative / losing_periods as f64
    } else {
        0.0
    };
    let payoff_ratio = if mean_losing != 0.0 {
        mean_winning / mean_losing.abs()
    } else if mean_winning > 0.0 {
        f64::INFINITY
    } else {
        0.0
    };

    // Kelly criterion: w - (1 - w) / b
    let kelly = if payoff_ratio > 0.0 && payoff_ratio.is_finite() {
        win_rate - (1.0 - win_rate) / payoff_ratio
    } else {
        0.0
    };

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
        cvar_95,
        win_rate,
        profit_factor,
        payoff_ratio,
        kelly,
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

/// Method for computing Conditional Value at Risk (a.k.a. Expected
/// Shortfall).
///
/// Different libraries use different conventions. This enum makes the
/// choice explicit. `Historical` is the default from v0.10; earlier
/// versions used `ParametricNormal` unconditionally.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum CVaRMethod {
    /// Pure empirical CVaR: sort the returns, take the lowest
    /// `ceil(n * alpha)` values, return their mean.
    ///
    /// Matches the standard academic convention and scipy-style
    /// percentile computations (`np.mean(np.sort(returns)[:ceil(n * alpha)])`).
    /// This is the v0.10 default.
    #[default]
    Historical,

    /// Hybrid estimator: compute a parametric VaR threshold assuming
    /// returns are `N(mean, sample_var)`, then return the empirical
    /// mean of returns strictly below that threshold.
    ///
    /// Matches `quantstats.stats.expected_shortfall` and nanobook's
    /// v0.9 behavior. The result coincides with `Historical` for
    /// well-behaved normal-ish samples but diverges on skewed,
    /// heavy-tailed, or small-sample data.
    ParametricNormal,
}

/// Conditional Value at Risk (a.k.a. Expected Shortfall) at tail
/// probability `alpha`.
///
/// For `alpha = 0.05`, returns the loss-level average of the worst
/// 5% of returns under the chosen [`CVaRMethod`]. Result is a
/// negative-signed return when the tail is negative.
///
/// Returns `0.0` for empty input or `alpha` outside `(0, 1)`.
pub fn cvar(returns: &[f64], alpha: f64, method: CVaRMethod) -> f64 {
    if returns.is_empty() || alpha <= 0.0 || alpha >= 1.0 {
        return 0.0;
    }
    match method {
        CVaRMethod::Historical => cvar_historical(returns, alpha),
        CVaRMethod::ParametricNormal => cvar_parametric(returns, alpha),
    }
}

/// Historical (empirical) CVaR: mean of the lowest `ceil(n * alpha)`
/// returns. Non-finite inputs are filtered out before sorting.
fn cvar_historical(returns: &[f64], alpha: f64) -> f64 {
    let mut sorted: Vec<f64> = returns.iter().copied().filter(|r| r.is_finite()).collect();
    if sorted.is_empty() {
        return 0.0;
    }
    sorted.sort_by(|a, b| a.partial_cmp(b).expect("finite values compare totally"));

    let tail_n = ((sorted.len() as f64) * alpha).ceil() as usize;
    let tail_n = tail_n.clamp(1, sorted.len());
    sorted[..tail_n].iter().sum::<f64>() / tail_n as f64
}

/// Hybrid parametric-normal CVaR: compute a parametric VaR threshold
/// assuming returns are `N(mean, sample_var)`, then take the empirical
/// mean of returns strictly below that threshold. Matches quantstats
/// and nanobook v0.9.
fn cvar_parametric(returns: &[f64], alpha: f64) -> f64 {
    let n = returns.len() as f64;
    let mu = returns.iter().sum::<f64>() / n;
    let var_pop = returns.iter().map(|&r| (r - mu).powi(2)).sum::<f64>() / (n - 1.0);
    let sigma = var_pop.sqrt();

    // Parametric VaR: norm.ppf(alpha, mu, sigma).
    // ppf(0.05) for standard normal ≈ -1.6448536269514729.
    let z = norm_ppf(alpha);
    let var_threshold = mu + sigma * z;

    let (tail_sum, tail_count) = returns
        .iter()
        .filter(|&&r| r < var_threshold)
        .fold((0.0_f64, 0_usize), |(sum, cnt), &r| (sum + r, cnt + 1));

    if tail_count == 0 {
        // Degenerate: no return crosses the parametric threshold. Fall
        // back to the single minimum return so the result is still a
        // meaningful loss level.
        return *returns
            .iter()
            .min_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap_or(&0.0);
    }
    tail_sum / tail_count as f64
}

/// Annualized Sortino ratio with configurable downside-deviation
/// degrees-of-freedom correction.
///
/// `ddof = 0` (used by [`compute_metrics`] from v0.10): downside
/// variance = `sum(min(excess_i, 0)²) / n`. Matches
/// `quantstats.stats.sortino` and the pandas `.std(ddof=0)` convention.
///
/// `ddof = 1`: Bessel-corrected downside variance uses `/ (n - 1)`.
/// This is the v0.9 default and the pandas `.std()` default.
///
/// # Returns
///
/// Annualized Sortino ratio. Returns `0.0` if:
/// - `returns` is empty.
/// - `ddof >= returns.len()` (would divide by zero or negative).
/// - `periods_per_year` is not strictly positive and finite.
/// - No return has strictly negative excess over `risk_free`
///   (downside deviation is zero).
///
/// # Notes
///
/// Excess returns are computed as `r - risk_free` per period. Downside
/// deviation uses only excess returns strictly below zero — gains are
/// penalised neither positively nor negatively.
pub fn sortino(returns: &[f64], risk_free: f64, periods_per_year: f64, ddof: u32) -> f64 {
    let n = returns.len();
    if n == 0 || (ddof as usize) >= n {
        return 0.0;
    }
    if !periods_per_year.is_finite() || periods_per_year <= 0.0 {
        return 0.0;
    }

    let mean = returns.iter().sum::<f64>() / n as f64;
    let excess_mean = mean - risk_free;

    let downside_sum: f64 = returns
        .iter()
        .map(|&r| {
            let excess = r - risk_free;
            if excess < 0.0 { excess.powi(2) } else { 0.0 }
        })
        .sum();

    let denom = (n - ddof as usize) as f64;
    let downside_dev = (downside_sum / denom).sqrt();

    if downside_dev > 0.0 {
        excess_mean * periods_per_year.sqrt() / downside_dev
    } else {
        0.0
    }
}

/// Inverse of the standard normal CDF (probit function).
///
/// Uses the rational approximation from Abramowitz & Stegun / Peter Acklam.
fn norm_ppf(p: f64) -> f64 {
    if p <= 0.0 {
        return f64::NEG_INFINITY;
    }
    if p >= 1.0 {
        return f64::INFINITY;
    }
    if (p - 0.5).abs() < 1e-15 {
        return 0.0;
    }

    // Rational approximation coefficients (Acklam) — exact values required for accuracy.
    #[allow(clippy::excessive_precision)]
    const A: [f64; 6] = [
        -3.969_683_028_665_376e1,
        2.209_460_984_245_205e2,
        -2.759_285_104_469_687e2,
        1.383_577_518_672_690e2,
        -3.066_479_806_614_716e1,
        2.506_628_277_459_239e0,
    ];
    const B: [f64; 5] = [
        -5.447_609_879_822_406e1,
        1.615_858_368_580_409e2,
        -1.556_989_798_598_866e2,
        6.680_131_188_771_972e1,
        -1.328_068_155_288_572e1,
    ];
    const C: [f64; 6] = [
        -7.784_894_002_430_293e-3,
        -3.223_964_580_411_365e-1,
        -2.400_758_277_161_838e0,
        -2.549_732_539_343_734e0,
        4.374_664_141_464_968e0,
        2.938_163_982_698_783e0,
    ];
    const D: [f64; 4] = [
        7.784_695_709_041_462e-3,
        3.224_671_290_700_398e-1,
        2.445_134_137_142_996e0,
        3.754_408_661_907_416e0,
    ];

    const P_LOW: f64 = 0.02425;
    const P_HIGH: f64 = 1.0 - P_LOW;

    if p < P_LOW {
        // Rational approximation for lower region
        let q = (-2.0 * p.ln()).sqrt();
        (((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    } else if p <= P_HIGH {
        // Rational approximation for central region
        let q = p - 0.5;
        let r = q * q;
        (((((A[0] * r + A[1]) * r + A[2]) * r + A[3]) * r + A[4]) * r + A[5]) * q
            / (((((B[0] * r + B[1]) * r + B[2]) * r + B[3]) * r + B[4]) * r + 1.0)
    } else {
        // Rational approximation for upper region
        let q = (-2.0 * (1.0 - p).ln()).sqrt();
        -(((((C[0] * q + C[1]) * q + C[2]) * q + C[3]) * q + C[4]) * q + C[5])
            / ((((D[0] * q + D[1]) * q + D[2]) * q + D[3]) * q + 1.0)
    }
}

/// Apply a function over a rolling window using O(N) running sum/sum-of-squares.
///
/// For each window starting at index `window - 1`, computes `(mean, m2)`
/// via Welford's online algorithm where `m2 = sum((x_i - mean)^2)`. The
/// closure receives `(mean, m2, k)` and returns the output value. Callers
/// pick the variance convention (population `m2 / k`, sample `m2 / (k - 1)`).
///
/// Positions before the first full window are filled with NaN.
///
/// # Numerical notes
///
/// Earlier versions maintained an O(1) sliding-sum state using
/// `sum += new - old; sum_sq += new*new - old*old;` and derived variance
/// via `sum_sq - sum^2/k`. That formula suffers catastrophic cancellation
/// on high-mean, low-variance series: both `sum_sq` and `sum^2/k` can be
/// large nearly-equal numbers whose difference is the (small) variance.
/// Rounding collapses the difference to zero, and downstream `.max(0.0)`
/// silently hides the error. On any stock trading above ~$500/share with
/// sub-cent ticks, `rolling_std_pop` returned exactly 0.
///
/// This rewrite recomputes Welford fresh per window — O(window) per step,
/// O(n*window) total. For typical financial windows (≤ 252) the overhead
/// is negligible. Reverse-Welford (O(1) eviction) is avoided because it
/// is itself unstable (Chan et al. 1983).
fn rolling_window(
    values: &[f64],
    window: usize,
    compute: impl Fn(f64, f64, f64) -> f64,
) -> Vec<f64> {
    let n = values.len();
    let mut out = vec![f64::NAN; n];
    if n < window || window < 2 {
        return out;
    }

    let k = window as f64;

    for i in (window - 1)..n {
        let slice = &values[i + 1 - window..=i];
        let (mean, m2) = crate::stats::welford_mean_m2(slice);
        out[i] = compute(mean, m2, k);
    }

    out
}

/// Rolling Sharpe ratio over a sliding window.
///
/// Returns NaN for positions where the window is incomplete.
///
/// # Arguments
///
/// * `returns` — Return series.
/// * `window` — Window size (e.g., 63 for quarterly).
/// * `periods_per_year` — Annualization factor (e.g., 252).
pub fn rolling_sharpe(returns: &[f64], window: usize, periods_per_year: usize) -> Vec<f64> {
    let ppy_sqrt = (periods_per_year as f64).sqrt();
    rolling_window(returns, window, |mean, m2, k| {
        let std = (m2 / (k - 1.0)).max(0.0).sqrt();
        if std > 0.0 {
            mean * ppy_sqrt / std
        } else {
            0.0
        }
    })
}

/// Rolling annualized volatility over a sliding window.
///
/// Returns NaN for positions where the window is incomplete.
///
/// # Arguments
///
/// * `returns` — Return series.
/// * `window` — Window size (e.g., 63 for quarterly).
/// * `periods_per_year` — Annualization factor (e.g., 252).
pub fn rolling_volatility(returns: &[f64], window: usize, periods_per_year: usize) -> Vec<f64> {
    let ppy_sqrt = (periods_per_year as f64).sqrt();
    rolling_window(returns, window, |_mean, m2, k| {
        (m2 / (k - 1.0)).max(0.0).sqrt() * ppy_sqrt
    })
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

    /// Regression for N13: non-positive `periods_per_year` used to silently
    /// produce `NaN` via `sqrt(≤0) / 0`. `compute_metrics` now rejects the
    /// input by returning `None`.
    #[test]
    fn compute_metrics_rejects_nonpositive_periods_per_year() {
        let returns = vec![0.01; 100];
        assert!(compute_metrics(&returns, 0.0, 0.0).is_none());
        assert!(compute_metrics(&returns, -1.0, 0.0).is_none());
        assert!(compute_metrics(&returns, f64::NAN, 0.0).is_none());
        assert!(compute_metrics(&returns, f64::INFINITY, 0.0).is_none());
        // Sanity: strictly positive still works.
        assert!(compute_metrics(&returns, 252.0, 0.0).is_some());
    }

    /// Regression for N13: standalone `sortino` returns `0.0` when
    /// `periods_per_year` is non-positive or non-finite, instead of the
    /// silent `NaN` produced by `sqrt` of a non-positive argument.
    #[test]
    fn sortino_returns_zero_on_nonpositive_periods_per_year() {
        // Positive-biased series: strictly positive Sortino when ppy > 0.
        let returns = vec![0.02, 0.01, -0.005, 0.015, -0.002, 0.012];
        assert_eq!(sortino(&returns, 0.0, 0.0, 0), 0.0);
        assert_eq!(sortino(&returns, 0.0, -1.0, 0), 0.0);
        assert!(sortino(&returns, 0.0, f64::NAN, 0) == 0.0);
        assert_eq!(sortino(&returns, 0.0, f64::INFINITY, 0), 0.0);
        // Sanity: strictly positive ppy produces a non-zero Sortino.
        assert!(sortino(&returns, 0.0, 252.0, 0) > 1e-12);
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
        assert!(s.contains("CVaR"));
        assert!(s.contains("Win rate:"));
        assert!(s.contains("Kelly:"));
    }

    // --- v0.8 extended metrics tests ---

    #[test]
    fn win_rate_all_positive() {
        let returns = vec![0.01, 0.02, 0.03];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!((m.win_rate - 1.0).abs() < 1e-10);
    }

    #[test]
    fn win_rate_half() {
        let returns = vec![0.01, -0.01, 0.01, -0.01];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!((m.win_rate - 0.5).abs() < 1e-10);
    }

    #[test]
    fn profit_factor_positive() {
        let returns = vec![0.02, -0.01, 0.03, -0.005];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        // sum_positive = 0.05, sum_negative = 0.015
        assert!(m.profit_factor > 1.0);
        assert!((m.profit_factor - 0.05 / 0.015).abs() < 1e-10);
    }

    #[test]
    fn profit_factor_all_positive() {
        let returns = vec![0.01, 0.02, 0.03];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!(m.profit_factor.is_infinite());
    }

    #[test]
    fn payoff_ratio() {
        let returns = vec![0.02, -0.01, 0.04, -0.02];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        // mean_winning = (0.02 + 0.04) / 2 = 0.03
        // mean_losing = (-0.01 + -0.02) / 2 = -0.015
        // payoff_ratio = 0.03 / 0.015 = 2.0
        assert!((m.payoff_ratio - 2.0).abs() < 1e-10);
    }

    #[test]
    fn kelly_criterion() {
        let returns = vec![0.02, -0.01, 0.04, -0.02];
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        // win_rate = 0.5, payoff_ratio = 2.0
        // kelly = 0.5 - (1-0.5)/2.0 = 0.5 - 0.25 = 0.25
        assert!((m.kelly - 0.25).abs() < 1e-10);
    }

    #[test]
    fn cvar_negative_tail() {
        // Returns with known negative tail
        let mut returns: Vec<f64> = vec![0.01; 95];
        returns.extend(vec![-0.10; 5]); // 5% worst = -10%
        let m = compute_metrics(&returns, 252.0, 0.0).unwrap();
        assert!(m.cvar_95 < 0.0, "CVaR should be negative");
        // CVaR should be approximately -0.10
        assert!((m.cvar_95 - (-0.10)).abs() < 0.01);
    }

    #[test]
    fn rolling_sharpe_basic() {
        let returns = vec![0.01; 100];
        let result = rolling_sharpe(&returns, 20, 252);
        assert_eq!(result.len(), 100);
        // First 19 should be NaN
        for v in result.iter().take(19) {
            assert!(v.is_nan());
        }
        // Constant returns → zero std → Sharpe = 0
        assert!(!result[19].is_nan());
    }

    #[test]
    fn rolling_volatility_basic() {
        let returns = vec![
            0.01, -0.01, 0.01, -0.01, 0.01, -0.01, 0.01, -0.01, 0.01, -0.01,
        ];
        let result = rolling_volatility(&returns, 5, 252);
        assert_eq!(result.len(), 10);
        assert!(result[3].is_nan());
        assert!(!result[4].is_nan());
        assert!(result[4] > 0.0);
    }
}
