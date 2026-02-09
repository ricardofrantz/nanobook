//! Statistical functions for quantitative analysis.
//!
//! Provides Spearman rank correlation and quintile spread analysis,
//! replacing direct scipy/numpy calls in qtrade.
//!
//! # References
//!
//! - SciPy `spearmanr`: <https://github.com/scipy/scipy/blob/main/scipy/stats/_correlation.py>
//! - Average-rank tie-breaking follows the standard convention.

// ---------------------------------------------------------------------------
// Ranking
// ---------------------------------------------------------------------------

/// Compute ranks with average tie-breaking (matches scipy's default).
///
/// Elements are ranked 1..N. Tied values receive the average of their ranks.
fn rankdata(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    if n == 0 {
        return vec![];
    }

    // Sort indices by value
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        values[a]
            .partial_cmp(&values[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let mut ranks = vec![0.0_f64; n];
    let mut i = 0;
    while i < n {
        // Find the extent of the tie group
        let mut j = i + 1;
        while j < n && values[indices[j]] == values[indices[i]] {
            j += 1;
        }

        // Average rank for this tie group (1-based)
        let avg_rank = (i + 1 + j) as f64 / 2.0;
        for &idx in &indices[i..j] {
            ranks[idx] = avg_rank;
        }
        i = j;
    }

    ranks
}

/// Pearson correlation coefficient between two slices.
fn pearson(x: &[f64], y: &[f64]) -> f64 {
    let n = x.len() as f64;
    if n < 2.0 {
        return f64::NAN;
    }

    let mean_x = x.iter().sum::<f64>() / n;
    let mean_y = y.iter().sum::<f64>() / n;

    let mut cov = 0.0_f64;
    let mut var_x = 0.0_f64;
    let mut var_y = 0.0_f64;

    for i in 0..x.len() {
        let dx = x[i] - mean_x;
        let dy = y[i] - mean_y;
        cov += dx * dy;
        var_x += dx * dx;
        var_y += dy * dy;
    }

    if var_x == 0.0 || var_y == 0.0 {
        return f64::NAN;
    }

    cov / (var_x * var_y).sqrt()
}

// ---------------------------------------------------------------------------
// t-distribution CDF (for p-value computation)
// ---------------------------------------------------------------------------

/// Regularized incomplete beta function I_x(a, b) via continued fraction.
///
/// Uses the symmetry relation I_x(a,b) = 1 - I_{1-x}(b,a) when x > a/(a+b)
/// to ensure the continued fraction converges quickly.
fn regularized_incomplete_beta(x: f64, a: f64, b: f64) -> f64 {
    if x <= 0.0 {
        return 0.0;
    }
    if x >= 1.0 {
        return 1.0;
    }

    // Use symmetry relation for convergence: when x > (a+1)/(a+b+2), flip.
    if x > (a + 1.0) / (a + b + 2.0) {
        return 1.0 - regularized_incomplete_beta_cf(1.0 - x, b, a);
    }
    regularized_incomplete_beta_cf(x, a, b)
}

/// Core evaluation of I_x(a, b) = front * betacf(a, b, x).
fn regularized_incomplete_beta_cf(x: f64, a: f64, b: f64) -> f64 {
    let ln_beta = ln_gamma(a) + ln_gamma(b) - ln_gamma(a + b);
    let front = (x.ln() * a + (1.0 - x).ln() * b - ln_beta).exp() / a;
    front * betacf(a, b, x)
}

/// Continued fraction for the incomplete beta function (Numerical Recipes).
fn betacf(a: f64, b: f64, x: f64) -> f64 {
    let tiny = 1e-30_f64;
    let eps = 3e-14;
    let max_iter = 200;

    let qab = a + b;
    let qap = a + 1.0;
    let qam = a - 1.0;

    let mut c = 1.0_f64;
    let mut d = 1.0 - qab * x / qap;
    if d.abs() < tiny {
        d = tiny;
    }
    d = 1.0 / d;
    let mut h = d;

    for m in 1..=max_iter {
        let em = m as f64;
        let m2 = 2.0 * em;

        // Even step
        let aa = em * (b - em) * x / ((qam + m2) * (a + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        h *= d * c;

        // Odd step
        let aa = -(a + em) * (qab + em) * x / ((a + m2) * (qap + m2));
        d = 1.0 + aa * d;
        if d.abs() < tiny {
            d = tiny;
        }
        c = 1.0 + aa / c;
        if c.abs() < tiny {
            c = tiny;
        }
        d = 1.0 / d;
        let del = d * c;
        h *= del;

        if (del - 1.0).abs() <= eps {
            return h;
        }
    }

    h // max iterations reached
}

/// Log-gamma function (Stirling's approximation + Lanczos).
fn ln_gamma(x: f64) -> f64 {
    // Lanczos approximation coefficients (g=7) — exact values required for accuracy.
    #[allow(clippy::excessive_precision)]
    let coefs = [
        0.999_999_999_999_809_93,
        676.520_368_121_885_1,
        -1259.139_216_722_402_8,
        771.323_428_777_653_08,
        -176.615_029_162_140_6,
        12.507_343_278_686_905,
        -0.138_571_095_265_720_12,
        9.984_369_578_019_572e-6,
        1.505_632_735_149_311_6e-7,
    ];

    if x < 0.5 {
        // Reflection formula
        let pi = std::f64::consts::PI;
        return pi.ln() - (pi * x).sin().ln() - ln_gamma(1.0 - x);
    }

    let x = x - 1.0;
    let mut sum = coefs[0];
    for (i, &c) in coefs[1..].iter().enumerate() {
        sum += c / (x + i as f64 + 1.0);
    }

    let t = x + 7.5; // g + 0.5
    0.5 * (2.0 * std::f64::consts::PI).ln() + (t.ln() * (x + 0.5)) - t + sum.ln()
}

/// Two-tailed p-value from t-statistic using t-distribution with `df` degrees of freedom.
fn t_distribution_two_tailed_p(t_stat: f64, df: f64) -> f64 {
    if df <= 0.0 {
        return f64::NAN;
    }
    let x = df / (df + t_stat * t_stat);
    let p_one_tail = 0.5 * regularized_incomplete_beta(x, df / 2.0, 0.5);
    2.0 * p_one_tail
}

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Spearman rank correlation coefficient with two-tailed p-value.
///
/// Matches scipy.stats.spearmanr behavior:
/// - Uses average-rank tie-breaking.
/// - P-value from t-distribution: `t = r * sqrt((n-2)/(1-r^2))`.
/// - Returns `(NaN, NaN)` if `n < 3`.
///
/// # Arguments
///
/// * `x`, `y` — Equal-length slices of observations.
///
/// # Returns
///
/// `(correlation, p_value)`
pub fn spearman(x: &[f64], y: &[f64]) -> (f64, f64) {
    let n = x.len();
    if n != y.len() || n < 3 {
        return (f64::NAN, f64::NAN);
    }

    let rank_x = rankdata(x);
    let rank_y = rankdata(y);
    let r = pearson(&rank_x, &rank_y);

    if r.is_nan() {
        return (f64::NAN, f64::NAN);
    }

    // Clamp to avoid NaN from sqrt of negative number at r = +/-1
    let r_clamped = r.clamp(-1.0, 1.0);
    if (r_clamped.abs() - 1.0).abs() < 1e-15 {
        return (r_clamped, 0.0);
    }

    let df = n as f64 - 2.0;
    let t_stat = r_clamped * (df / (1.0 - r_clamped * r_clamped)).sqrt();
    let p_value = t_distribution_two_tailed_p(t_stat, df);

    (r_clamped, p_value)
}

/// Quintile spread: mean of top quintile returns minus mean of bottom quintile returns.
///
/// Sorts observations by `scores`, splits into `n_quantiles` groups, and returns
/// the difference between the mean of the top group's `returns` and the bottom group's.
///
/// # Arguments
///
/// * `scores` — Factor scores (higher = better expected return).
/// * `returns` — Realized returns corresponding to each score.
/// * `n_quantiles` — Number of groups (typically 5 for quintiles).
///
/// # Returns
///
/// `top_mean - bottom_mean`, or NaN if inputs are invalid.
pub fn quintile_spread(scores: &[f64], returns: &[f64], n_quantiles: usize) -> f64 {
    let n = scores.len();
    if n != returns.len() || n < n_quantiles || n_quantiles == 0 {
        return f64::NAN;
    }

    // Sort indices by score (ascending)
    let mut indices: Vec<usize> = (0..n).collect();
    indices.sort_by(|&a, &b| {
        scores[a]
            .partial_cmp(&scores[b])
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let group_size = n / n_quantiles;
    if group_size == 0 {
        return f64::NAN;
    }

    // Bottom group (lowest scores)
    let bottom_mean: f64 = indices[..group_size]
        .iter()
        .map(|&i| returns[i])
        .sum::<f64>()
        / group_size as f64;

    // Top group (highest scores)
    let top_start = n - group_size;
    let top_mean: f64 = indices[top_start..]
        .iter()
        .map(|&i| returns[i])
        .sum::<f64>()
        / group_size as f64;

    top_mean - bottom_mean
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rankdata_no_ties() {
        let values = [3.0, 1.0, 2.0];
        let ranks = rankdata(&values);
        assert!((ranks[0] - 3.0).abs() < 1e-10);
        assert!((ranks[1] - 1.0).abs() < 1e-10);
        assert!((ranks[2] - 2.0).abs() < 1e-10);
    }

    #[test]
    fn rankdata_with_ties() {
        let values = [1.0, 2.0, 2.0, 4.0];
        let ranks = rankdata(&values);
        assert!((ranks[0] - 1.0).abs() < 1e-10);
        assert!((ranks[1] - 2.5).abs() < 1e-10); // tied → average
        assert!((ranks[2] - 2.5).abs() < 1e-10);
        assert!((ranks[3] - 4.0).abs() < 1e-10);
    }

    #[test]
    fn rankdata_empty() {
        let ranks = rankdata(&[]);
        assert!(ranks.is_empty());
    }

    #[test]
    fn spearman_perfect_positive() {
        let x: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let (r, p) = spearman(&x, &y);
        assert!((r - 1.0).abs() < 1e-10, "expected r=1.0, got {r}");
        assert!(p < 1e-10, "expected p≈0, got {p}");
    }

    #[test]
    fn spearman_perfect_negative() {
        let x: Vec<f64> = (0..50).map(|i| i as f64).collect();
        let y: Vec<f64> = (0..50).rev().map(|i| i as f64).collect();
        let (r, p) = spearman(&x, &y);
        assert!((r - (-1.0)).abs() < 1e-10, "expected r=-1.0, got {r}");
        assert!(p < 1e-10, "expected p≈0, got {p}");
    }

    #[test]
    fn spearman_too_few() {
        let x = [1.0, 2.0];
        let y = [3.0, 4.0];
        let (r, p) = spearman(&x, &y);
        assert!(r.is_nan());
        assert!(p.is_nan());
    }

    #[test]
    fn spearman_unequal_length() {
        let x = [1.0, 2.0, 3.0];
        let y = [1.0, 2.0];
        let (r, _) = spearman(&x, &y);
        assert!(r.is_nan());
    }

    #[test]
    fn quintile_spread_basic() {
        // Scores: 1..10, Returns match scores → positive spread
        let scores: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let returns: Vec<f64> = (1..=10).map(|i| i as f64 * 0.01).collect();
        let spread = quintile_spread(&scores, &returns, 5);
        assert!(spread > 0.0, "expected positive spread, got {spread}");
    }

    #[test]
    fn quintile_spread_zero_for_random() {
        // Scores inversely related to returns → negative spread
        let scores: Vec<f64> = (1..=10).map(|i| i as f64).collect();
        let returns: Vec<f64> = (1..=10).rev().map(|i| i as f64 * 0.01).collect();
        let spread = quintile_spread(&scores, &returns, 5);
        assert!(spread < 0.0, "expected negative spread, got {spread}");
    }

    #[test]
    fn quintile_spread_invalid() {
        let scores = [1.0, 2.0];
        let returns = [0.01, 0.02];
        let spread = quintile_spread(&scores, &returns, 5);
        assert!(spread.is_nan());
    }

    #[test]
    fn ln_gamma_known_values() {
        // ln(Gamma(1)) = 0
        assert!(ln_gamma(1.0).abs() < 1e-10);
        // ln(Gamma(2)) = 0 (since Gamma(2) = 1! = 1)
        assert!(ln_gamma(2.0).abs() < 1e-10);
        // Gamma(5) = 24, ln(24) ≈ 3.178
        assert!((ln_gamma(5.0) - 24.0_f64.ln()).abs() < 1e-8);
    }
}
