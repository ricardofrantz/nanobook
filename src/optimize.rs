//! Long-only portfolio optimizers used by the Python bridge.
//!
//! The implementations here are deterministic and safety-first:
//! - invalid inputs return empty weights,
//! - valid outputs are finite, non-negative, and sum to ~1.

/// Long-only minimum-variance optimization on the unit simplex.
pub fn optimize_min_variance(returns: &[Vec<f64>]) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    if cols == 1 {
        return vec![1.0];
    }

    let cov = covariance_matrix(returns);
    let mut w = equal_weights(cols);
    let mut lr = 0.20_f64;

    for _ in 0..350 {
        let sigma_w = mat_vec_mul(&cov, &w);
        let grad: Vec<f64> = sigma_w.iter().map(|g| 2.0 * g).collect();
        let candidate: Vec<f64> = w.iter().zip(&grad).map(|(wi, gi)| wi - lr * gi).collect();
        let projected = project_simplex(&candidate);

        if squared_distance(&projected, &w) < 1e-16 {
            w = projected;
            break;
        }

        w = projected;
        lr *= 0.995;
    }

    normalize_long_only(w)
}

/// Long-only maximum-Sharpe optimization on the unit simplex.
pub fn optimize_max_sharpe(returns: &[Vec<f64>], risk_free: f64) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    if cols == 1 {
        return vec![1.0];
    }

    let mu = column_means(returns);
    let excess: Vec<f64> = mu.into_iter().map(|m| m - risk_free).collect();

    if excess.iter().all(|x| *x <= 0.0 || !x.is_finite()) {
        return optimize_min_variance(returns);
    }

    let cov = covariance_matrix(returns);
    let mut w = equal_weights(cols);
    let mut lr = 0.08_f64;

    for _ in 0..450 {
        let sigma_w = mat_vec_mul(&cov, &w);
        let var = dot(&w, &sigma_w).max(1e-12);
        let vol = var.sqrt();
        let num = dot(&w, &excess);

        let grad: Vec<f64> = excess
            .iter()
            .zip(&sigma_w)
            .map(|(a, sw)| a / vol - num * sw / (var * vol))
            .collect();

        // Gradient ascent on Sharpe objective, then project.
        let candidate: Vec<f64> = w.iter().zip(&grad).map(|(wi, gi)| wi + lr * gi).collect();
        let projected = project_simplex(&candidate);

        if squared_distance(&projected, &w) < 1e-16 {
            w = projected;
            break;
        }

        w = projected;
        lr *= 0.995;
    }

    normalize_long_only(w)
}

/// Long-only risk parity approximation.
pub fn optimize_risk_parity(returns: &[Vec<f64>]) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    if cols == 1 {
        return vec![1.0];
    }

    let cov = covariance_matrix(returns);
    let mut w = equal_weights(cols);

    for _ in 0..600 {
        let sigma_w = mat_vec_mul(&cov, &w);
        let port_var = dot(&w, &sigma_w).max(1e-12);
        let target = port_var / cols as f64;

        let mut next = vec![0.0; cols];
        for i in 0..cols {
            let rc = (w[i] * sigma_w[i]).abs().max(1e-12);
            let update = w[i] * (target / rc).sqrt();
            next[i] = if update.is_finite() {
                update.max(0.0)
            } else {
                0.0
            };
        }

        next = normalize_long_only(next);

        // Damping stabilizes oscillations on near-singular covariance matrices.
        let damped: Vec<f64> = w
            .iter()
            .zip(&next)
            .map(|(old, new)| 0.6 * old + 0.4 * new)
            .collect();
        let damped = normalize_long_only(damped);

        if squared_distance(&damped, &w) < 1e-16 {
            w = damped;
            break;
        }

        w = damped;
    }

    normalize_long_only(w)
}

/// Compute long-only weights inversely proportional to each asset's per-asset
/// CVaR.
///
/// This is a heuristic: it does NOT minimize portfolio-level CVaR because
/// cross-asset covariance is ignored. For true LP-based minimization, use
/// Python's `cvxpy` with the Rockafellar-Uryasev formulation, or wait for the
/// `cvar-lp` feature flag in nanobook >= 0.11.
pub fn inverse_cvar_weights(returns: &[Vec<f64>], alpha: f64) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    if cols == 1 {
        return vec![1.0];
    }

    let cols_data = columns(returns);
    let alpha = alpha.clamp(0.5, 0.999);

    let risks: Vec<f64> = cols_data
        .iter()
        .map(|col| asset_cvar(col, alpha).max(1e-8))
        .collect();

    inverse_risk_weights(&risks)
}

/// Compute long-only weights inversely proportional to each asset's per-asset
/// Conditional Drawdown at Risk (CDaR).
///
/// Same heuristic semantics as [`inverse_cvar_weights`]: this does NOT
/// minimize portfolio-level CDaR because cross-asset covariance is ignored.
pub fn inverse_cdar_weights(returns: &[Vec<f64>], alpha: f64) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    if cols == 1 {
        return vec![1.0];
    }

    let cols_data = columns(returns);
    let alpha = alpha.clamp(0.5, 0.999);

    let risks: Vec<f64> = cols_data
        .iter()
        .map(|col| asset_cdar(col, alpha).max(1e-8))
        .collect();

    inverse_risk_weights(&risks)
}

/// Deprecated alias for [`inverse_cvar_weights`]; removed in v0.11.
#[rustfmt::skip]
#[deprecated(since = "0.9.3", note = "use `inverse_cvar_weights`; this is a heuristic, not a CVaR LP solver")]
pub fn optimize_cvar(returns: &[Vec<f64>], alpha: f64) -> Vec<f64> {
    inverse_cvar_weights(returns, alpha)
}

/// Deprecated alias for [`inverse_cdar_weights`]; removed in v0.11.
#[rustfmt::skip]
#[deprecated(since = "0.9.3", note = "use `inverse_cdar_weights`; this is a heuristic, not a CDaR LP solver")]
pub fn optimize_cdar(returns: &[Vec<f64>], alpha: f64) -> Vec<f64> {
    inverse_cdar_weights(returns, alpha)
}

fn matrix_shape(matrix: &[Vec<f64>]) -> Option<(usize, usize)> {
    let rows = matrix.len();
    if rows < 2 {
        return None;
    }

    let cols = matrix.first()?.len();
    if cols == 0 {
        return None;
    }

    for row in matrix {
        if row.len() != cols || row.iter().any(|x| !x.is_finite()) {
            return None;
        }
    }

    Some((rows, cols))
}

fn column_means(matrix: &[Vec<f64>]) -> Vec<f64> {
    let rows = matrix.len();
    let cols = matrix[0].len();

    let mut sums = vec![0.0; cols];
    for row in matrix {
        for (j, v) in row.iter().enumerate() {
            sums[j] += *v;
        }
    }

    sums.into_iter().map(|s| s / rows as f64).collect()
}

fn covariance_matrix(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let rows = matrix.len();
    let cols = matrix[0].len();
    let means = column_means(matrix);

    let mut cov = vec![vec![0.0; cols]; cols];

    for row in matrix {
        for i in 0..cols {
            let di = row[i] - means[i];
            for j in i..cols {
                let dj = row[j] - means[j];
                cov[i][j] += di * dj;
            }
        }
    }

    let denom = (rows as f64 - 1.0).max(1.0);
    #[allow(clippy::needless_range_loop)]
    for i in 0..cols {
        for j in i..cols {
            let v = cov[i][j] / denom;
            cov[i][j] = v;
            cov[j][i] = v;
        }
    }

    // Relative ridge for numerical stability.
    //
    // The diagonal is scaled by `1e-6 * trace(Σ) / n`, i.e. a fixed
    // fraction of the mean variance. A fixed `1e-10 * I` is too small
    // for daily-return covariances whose eigenvalues can be O(10⁻⁴) or
    // smaller, leaving the matrix effectively singular for correlated
    // assets. Scaling with the trace keeps the ridge meaningful across
    // unit choices (daily vs. monthly returns, percent vs. decimal).
    // Computed BEFORE mutation so the ridge does not inflate itself.
    let trace: f64 = (0..cols).map(|i| cov[i][i]).sum();
    let n = cols as f64;
    let ridge = if trace > 0.0 && trace.is_finite() {
        1e-6 * trace / n
    } else {
        // Degenerate input (zero variance everywhere or NaN/Inf):
        // fall back to the legacy absolute ridge so downstream
        // solvers still see a strictly positive definite matrix.
        1e-10
    };
    for (i, row) in cov.iter_mut().enumerate() {
        row[i] += ridge;
    }

    cov
}

fn columns(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let rows = matrix.len();
    let cols = matrix[0].len();
    let mut out = vec![vec![0.0; rows]; cols];

    for (i, row) in matrix.iter().enumerate() {
        for (j, v) in row.iter().enumerate() {
            out[j][i] = *v;
        }
    }

    out
}

fn mat_vec_mul(matrix: &[Vec<f64>], vec: &[f64]) -> Vec<f64> {
    matrix
        .iter()
        .map(|row| row.iter().zip(vec).map(|(a, b)| a * b).sum::<f64>())
        .collect()
}

fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn squared_distance(a: &[f64], b: &[f64]) -> f64 {
    a.iter()
        .zip(b)
        .map(|(x, y)| {
            let d = x - y;
            d * d
        })
        .sum::<f64>()
}

fn equal_weights(n: usize) -> Vec<f64> {
    if n == 0 {
        return Vec::new();
    }
    vec![1.0 / n as f64; n]
}

fn normalize_long_only(mut w: Vec<f64>) -> Vec<f64> {
    if w.is_empty() {
        return w;
    }

    for x in &mut w {
        if !x.is_finite() || *x < 0.0 {
            *x = 0.0;
        }
    }

    let sum = w.iter().sum::<f64>();
    if sum <= 1e-12 {
        return equal_weights(w.len());
    }

    for x in &mut w {
        *x /= sum;
    }
    w
}

fn project_simplex(v: &[f64]) -> Vec<f64> {
    if v.is_empty() {
        return Vec::new();
    }

    let mut u = v.to_vec();
    u.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let mut cssv = 0.0;
    let mut rho = 0_usize;

    for (i, ui) in u.iter().enumerate() {
        cssv += *ui;
        let theta = (cssv - 1.0) / (i as f64 + 1.0);
        if *ui - theta > 0.0 {
            rho = i + 1;
        }
    }

    if rho == 0 {
        return equal_weights(v.len());
    }

    let theta = (u[..rho].iter().sum::<f64>() - 1.0) / rho as f64;
    let projected: Vec<f64> = v.iter().map(|x| (x - theta).max(0.0)).collect();
    normalize_long_only(projected)
}

fn inverse_risk_weights(risks: &[f64]) -> Vec<f64> {
    if risks.is_empty() {
        return Vec::new();
    }

    let scores: Vec<f64> = risks
        .iter()
        .map(|r| {
            let rr = if r.is_finite() && *r > 0.0 { *r } else { 1.0 };
            1.0 / rr
        })
        .collect();
    normalize_long_only(scores)
}

fn tail_count(n: usize, alpha: f64) -> usize {
    let tail = ((1.0 - alpha) * n as f64).ceil() as usize;
    tail.clamp(1, n)
}

fn asset_cvar(returns: &[f64], alpha: f64) -> f64 {
    let mut losses: Vec<f64> = returns.iter().map(|r| (-r).max(0.0)).collect();
    losses.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let k = tail_count(losses.len(), alpha);
    losses.iter().take(k).sum::<f64>() / k as f64
}

fn asset_cdar(returns: &[f64], alpha: f64) -> f64 {
    let mut equity = 1.0_f64;
    let mut peak = 1.0_f64;
    let mut drawdowns = Vec::with_capacity(returns.len());

    for r in returns {
        let growth = (1.0 + r).max(1e-9);
        equity *= growth;
        if equity > peak {
            peak = equity;
        }
        let dd = ((peak - equity) / peak).max(0.0);
        drawdowns.push(dd);
    }

    drawdowns.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let k = tail_count(drawdowns.len(), alpha);
    drawdowns.iter().take(k).sum::<f64>() / k as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_returns() -> Vec<Vec<f64>> {
        vec![
            vec![0.010, 0.004, -0.002],
            vec![-0.003, 0.006, 0.001],
            vec![0.007, -0.001, 0.002],
            vec![0.004, 0.003, -0.004],
            vec![-0.002, 0.005, 0.003],
            vec![0.006, -0.002, 0.001],
            vec![0.003, 0.004, -0.001],
            vec![-0.001, 0.002, 0.002],
        ]
    }

    fn qtrade_reference_returns() -> Vec<Vec<f64>> {
        vec![
            vec![0.010, 0.004, -0.002, 0.006],
            vec![-0.003, 0.006, 0.001, -0.002],
            vec![0.007, -0.001, 0.002, 0.004],
            vec![0.004, 0.003, -0.004, 0.005],
            vec![-0.002, 0.005, 0.003, -0.001],
            vec![0.006, -0.002, 0.001, 0.003],
            vec![0.003, 0.004, -0.001, 0.002],
            vec![-0.001, 0.002, 0.002, -0.003],
            vec![0.005, 0.001, -0.002, 0.004],
            vec![0.002, 0.003, 0.001, 0.000],
            vec![-0.004, 0.002, 0.003, -0.002],
            vec![0.006, -0.001, 0.000, 0.005],
        ]
    }

    fn assert_valid_weights(w: &[f64], n: usize) {
        assert_eq!(w.len(), n);
        assert!(w.iter().all(|x| x.is_finite() && *x >= -1e-12));
        let s: f64 = w.iter().sum();
        assert!((s - 1.0).abs() < 1e-8, "sum={s}");
    }

    #[test]
    fn min_variance_weights_are_valid() {
        let r = sample_returns();
        let w = optimize_min_variance(&r);
        assert_valid_weights(&w, 3);
    }

    /// Regression for N9: the covariance ridge is a fraction of
    /// `trace(Σ) / n`, not a fixed absolute constant. With `σ = 0.1`
    /// (10% returns) the ridge must be O(10⁻⁸) — the old fixed
    /// `1e-10` would be off by two orders of magnitude.
    #[test]
    fn covariance_ridge_is_trace_relative() {
        // Two assets with iid returns of σ ≈ 0.1. Diagonal ≈ 0.01.
        let r: Vec<Vec<f64>> = vec![
            vec![0.10, -0.10],
            vec![-0.10, 0.10],
            vec![0.10, -0.10],
            vec![-0.10, 0.10],
            vec![0.10, -0.10],
        ];
        let cov = covariance_matrix(&r);

        // Pre-ridge diagonal = sample variance (same for both assets).
        // Col 1: [0.10, -0.10, 0.10, -0.10, 0.10], mean = 0.02.
        // Squared deviations sum = 3*(0.08)² + 2*(0.12)² = 0.0192 + 0.0288 = 0.048.
        // σ² = 0.048 / 4 = 0.012.
        let expected_var = 0.012;
        // Trace(Σ) / n = expected_var; ridge = 1e-6 * expected_var.
        let expected_ridge = 1e-6 * expected_var;
        let expected_diag = expected_var + expected_ridge;

        assert!(
            (cov[0][0] - expected_diag).abs() < 1e-12,
            "cov[0][0] = {}, expected {}",
            cov[0][0],
            expected_diag
        );
        // Ridge contribution on its own must be O(1e-8) for this scale —
        // more than 10x the legacy 1e-10 absolute ridge.
        let ridge_added = cov[0][0] - expected_var;
        assert!(
            ridge_added > 10.0 * 1e-10,
            "trace-relative ridge {ridge_added} should dominate legacy 1e-10 at σ=0.1"
        );
    }

    /// Regression for N9 (coverage): perfectly correlated assets still
    /// produce finite, simplex-valid min-variance weights.
    #[test]
    fn min_variance_on_perfectly_correlated_assets_returns_finite() {
        // One underlying daily-return series with σ ≈ 1%.
        let factor: Vec<f64> = vec![
            0.010, -0.003, 0.007, 0.004, -0.002, 0.006, 0.003, -0.001, 0.005, 0.002, -0.004, 0.006,
        ];
        // Three identical columns — maximum correlation.
        let returns: Vec<Vec<f64>> = factor.iter().map(|&r| vec![r, r, r]).collect();

        let w = optimize_min_variance(&returns);

        assert_eq!(w.len(), 3);
        assert!(
            w.iter().all(|v| v.is_finite()),
            "non-finite weight: {:?}",
            w
        );
        assert!(w.iter().all(|v| *v >= -1e-12), "negative weight: {:?}", w);
        let s: f64 = w.iter().sum();
        assert!(
            (s - 1.0).abs() < 1e-6,
            "weights must sum to 1, got {s} ({:?})",
            w
        );
    }

    #[test]
    fn max_sharpe_weights_are_valid() {
        let r = sample_returns();
        let w = optimize_max_sharpe(&r, 0.0);
        assert_valid_weights(&w, 3);
    }

    #[test]
    fn risk_parity_weights_are_valid() {
        let r = sample_returns();
        let w = optimize_risk_parity(&r);
        assert_valid_weights(&w, 3);
    }

    #[test]
    fn inverse_cvar_weights_are_valid() {
        let r = sample_returns();
        let w = inverse_cvar_weights(&r, 0.95);
        assert_valid_weights(&w, 3);
    }

    #[test]
    fn inverse_cdar_weights_are_valid() {
        let r = sample_returns();
        let w = inverse_cdar_weights(&r, 0.95);
        assert_valid_weights(&w, 3);
    }

    #[allow(deprecated)]
    #[test]
    fn deprecated_cvar_cdar_shims_delegate() {
        let r = sample_returns();

        assert_eq!(optimize_cvar(&r, 0.95), inverse_cvar_weights(&r, 0.95));
        assert_eq!(optimize_cdar(&r, 0.95), inverse_cdar_weights(&r, 0.95));
    }

    #[test]
    fn invalid_matrix_returns_empty() {
        let bad = vec![vec![0.01, 0.02], vec![0.03]];
        assert!(optimize_min_variance(&bad).is_empty());
    }

    fn assert_close(got: &[f64], expected: &[f64], atol: f64) {
        assert_eq!(got.len(), expected.len());
        for (g, e) in got.iter().zip(expected.iter()) {
            assert!((*g - *e).abs() <= atol, "got={g} expected={e}");
        }
    }

    // Literals are Rust's Debug round-trip form — intentionally precise.
    #[allow(clippy::excessive_precision)]
    #[test]
    fn qtrade_reference_fixture_targets() {
        let r = qtrade_reference_returns();

        let minvar = optimize_min_variance(&r);
        let maxsh = optimize_max_sharpe(&r, 0.0);
        let rp = optimize_risk_parity(&r);
        let cvar = inverse_cvar_weights(&r, 0.95);
        let cdar = inverse_cdar_weights(&r, 0.95);

        // Goldens regenerated in N9 when the covariance ridge moved from
        // a fixed `1e-10 * I` to the trace-relative `(1e-6 * trace(Σ)/n) * I`.
        // At the qtrade fixture's scale (σ ≈ 0.3% daily), the new ridge is
        // ~1e-11 — smaller than the legacy 1e-10, so the optimizer lands
        // closer to the un-regularized fixed point. CVaR / CDaR don't use
        // the covariance matrix and are unchanged.
        assert_close(
            &minvar,
            &[
                0.24975737320731933,
                0.25015997245484023,
                0.25021559627060619,
                0.24986705806723414,
            ],
            5e-13,
        );
        assert_close(
            &maxsh,
            &[
                0.06213077960176228,
                0.30352368598709623,
                0.38160955623207826,
                0.25273597817906324,
            ],
            5e-13,
        );
        assert_close(
            &rp,
            &[
                0.11767968124665359,
                0.27473046110881050,
                0.45096650611370881,
                0.15662335153082713,
            ],
            5e-13,
        );
        assert_close(&cvar, &[0.1875, 0.3750, 0.1875, 0.2500], 1e-15);
        assert_close(&cdar, &[0.1875, 0.3750, 0.1875, 0.2500], 1e-12);
    }
}
