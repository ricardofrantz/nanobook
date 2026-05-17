//! Long-only portfolio optimizers used by the Python bridge.
//!
//! The implementations here are deterministic and safety-first:
//! - invalid inputs return empty weights,
//! - valid outputs are finite, non-negative, and sum to ~1.

/// Errors returned by helpers in this module.
///
/// The high-level optimizers (`optimize_min_variance`, `optimize_max_sharpe`,
/// etc.) swallow these and fall back to their own safe defaults; the error
/// variants surface only through direct calls to low-level primitives
/// like [`project_simplex`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum OptimizeError {
    /// Empty input slice — no projection to compute.
    EmptyInput,
    /// The input vector has no positive finite component, so a simplex
    /// projection would silently produce equal weights. Surfacing this as
    /// an error prevents masking upstream convergence failures (e.g. an
    /// optimizer that converged to a zero gradient).
    DegenerateProjection,
}

impl core::fmt::Display for OptimizeError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::EmptyInput => f.write_str("empty input"),
            Self::DegenerateProjection => {
                f.write_str("simplex projection is degenerate (no positive finite component)")
            }
        }
    }
}

impl std::error::Error for OptimizeError {}

/// Tunable parameters for the projected-gradient optimizers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptimizerOptions {
    /// Maximum projected-gradient iterations. Execution stops early if
    /// the squared step distance drops below `tol`.
    pub max_iters: usize,
    /// Convergence tolerance on the squared L2 distance between
    /// successive iterates (`‖wₖ₊₁ − wₖ‖²`).
    pub tol: f64,
}

impl Default for OptimizerOptions {
    /// Backward-compatible defaults: 350 iterations, `tol = 1e-16`.
    /// In practice `1e-16` on the squared step rarely triggers on
    /// daily-return data — the budget dominates. Tighten `max_iters`
    /// for speed, loosen `tol` for early stopping.
    fn default() -> Self {
        Self {
            max_iters: 350,
            tol: 1e-16,
        }
    }
}

/// Weights plus diagnostics returned by the `_ex` optimizer variants.
///
/// `converged == true` iff the squared step between consecutive iterates
/// fell below `OptimizerOptions.tol` before `max_iters` was reached. On
/// a budget-exhausted run `converged == false` and `iters == max_iters`;
/// the weights are still the last projected iterate (normalized).
#[derive(Clone, Debug, PartialEq)]
pub struct OptimizerResult {
    /// Long-only simplex weights from the final iterate.
    pub weights: Vec<f64>,
    /// Number of inner-loop iterations actually performed.
    pub iters: usize,
    /// True if `final_step_squared < tol` triggered early termination.
    pub converged: bool,
    /// Squared L2 distance between the last two iterates. `f64::NAN`
    /// when the loop exited before any iteration completed (e.g., a
    /// single-asset shortcut or degenerate input).
    pub final_step_squared: f64,
}

/// Long-only minimum-variance optimization on the unit simplex.
///
/// Backward-compatible wrapper around [`optimize_min_variance_ex`] with
/// [`OptimizerOptions::default`]. Returns weights only — callers that
/// need convergence diagnostics should call the `_ex` variant directly.
pub fn optimize_min_variance(returns: &[Vec<f64>]) -> Vec<f64> {
    optimize_min_variance_ex(returns, OptimizerOptions::default()).weights
}

/// Long-only minimum-variance optimization on the unit simplex, with
/// convergence diagnostics.
///
/// Runs projected gradient descent on `½ wᵀΣw` under the simplex
/// constraint. On invalid input (empty matrix, inconsistent row
/// lengths, non-finite entries) returns an empty-weights result with
/// `converged = false` and `iters = 0`. On single-asset input returns
/// `[1.0]` with `converged = true`.
pub fn optimize_min_variance_ex(
    returns: &[Vec<f64>],
    options: OptimizerOptions,
) -> OptimizerResult {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return OptimizerResult {
            weights: Vec::new(),
            iters: 0,
            converged: false,
            final_step_squared: f64::NAN,
        };
    };

    if cols == 1 {
        return OptimizerResult {
            weights: vec![1.0],
            iters: 0,
            converged: true,
            final_step_squared: f64::NAN,
        };
    }

    let cov = covariance_matrix(returns);
    let mut w = equal_weights(cols);
    let mut lr = 0.20_f64;
    let mut iters = 0_usize;
    let mut converged = false;
    let mut last_step_sq = f64::NAN;

    for _ in 0..options.max_iters {
        let sigma_w = mat_vec_mul(&cov, &w);
        let grad: Vec<f64> = sigma_w.iter().map(|g| 2.0 * g).collect();
        let candidate: Vec<f64> = w.iter().zip(&grad).map(|(wi, gi)| wi - lr * gi).collect();
        // A degenerate projection means the gradient step landed on a
        // zero/non-finite iterate — keep the last good weights.
        let projected = match project_simplex(&candidate) {
            Ok(p) => p,
            Err(_) => break,
        };

        let step_sq = squared_distance(&projected, &w);
        iters += 1;
        last_step_sq = step_sq;

        if step_sq < options.tol {
            w = projected;
            converged = true;
            break;
        }

        w = projected;
        lr *= 0.995;
    }

    OptimizerResult {
        weights: normalize_long_only(w),
        iters,
        converged,
        final_step_squared: last_step_sq,
    }
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
        // A degenerate projection means the gradient step zeroed the
        // iterate — keep the last good weights.
        let projected = match project_simplex(&candidate) {
            Ok(p) => p,
            Err(_) => break,
        };

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

/// One merge step in an agglomerative clustering dendrogram.
///
/// Leaves are indexed `0..n_assets`; internal clusters are assigned IDs
/// `n_assets + merge_index`, matching scipy-style linkage conventions.
#[derive(Clone, Debug, PartialEq)]
pub struct LinkageMerge {
    /// Left child cluster ID.
    pub left: usize,
    /// Right child cluster ID.
    pub right: usize,
    /// Single-linkage distance at which the children merged.
    pub distance: f64,
    /// Number of original assets contained in the merged cluster.
    pub size: usize,
}

/// Compute the sample correlation matrix for a returns matrix.
///
/// Rows are time periods and columns are assets. Invalid input returns an
/// empty matrix. The diagonal is pinned to `1.0`; off-diagonal entries with
/// zero or non-finite variance are set to `0.0`. Uses raw covariance
/// (without ridge regularization) to avoid distorting clustering structure.
pub fn correlation_matrix(returns: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };

    let cov = raw_covariance_matrix(returns);
    let mut corr = vec![vec![0.0; cols]; cols];

    for i in 0..cols {
        for j in i..cols {
            let value = if i == j {
                1.0
            } else {
                let denom = (cov[i][i] * cov[j][j]).sqrt();
                if denom.is_finite() && denom > 1e-12 {
                    (cov[i][j] / denom).clamp(-1.0, 1.0)
                } else {
                    0.0
                }
            };
            corr[i][j] = value;
            corr[j][i] = value;
        }
    }

    corr
}

/// Convert a correlation matrix into López de Prado's clustering distance.
///
/// Uses `d[i][j] = sqrt(0.5 * (1 - corr[i][j]))`. Invalid or non-square input
/// returns an empty matrix.
fn distance_matrix(correlation: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let n = correlation.len();
    if n == 0 || correlation.iter().any(|row| row.len() != n) {
        return Vec::new();
    }

    let mut dist = vec![vec![0.0; n]; n];
    for i in 0..n {
        for j in i..n {
            let value = if i == j {
                0.0
            } else {
                let corr = if correlation[i][j].is_finite() {
                    correlation[i][j].clamp(-1.0, 1.0)
                } else {
                    0.0
                };
                (0.5 * (1.0 - corr)).max(0.0).sqrt()
            };
            dist[i][j] = value;
            dist[j][i] = value;
        }
    }

    dist
}

/// Build a single-linkage hierarchical clustering dendrogram.
///
/// The input is a square distance matrix over assets. At each step the two
/// clusters with the smallest pairwise asset distance are merged.
///
/// Performance note: This implementation is O(n³) in the number of assets,
/// which is acceptable for typical portfolio sizes (n < 50). For very large
/// portfolios (n > 100), consider using a priority-queue based approach
/// for O(n² log n) performance.
fn single_linkage_clustering(distance: &[Vec<f64>]) -> Vec<LinkageMerge> {
    let n = distance.len();
    if n < 2 || distance.iter().any(|row| row.len() != n) {
        return Vec::new();
    }

    #[derive(Clone)]
    struct Cluster {
        id: usize,
        members: Vec<usize>,
    }

    let mut clusters: Vec<Cluster> = (0..n)
        .map(|i| Cluster {
            id: i,
            members: vec![i],
        })
        .collect();
    let mut linkage = Vec::with_capacity(n - 1);
    let mut next_id = n;

    while clusters.len() > 1 {
        let mut best = (0_usize, 1_usize, f64::INFINITY);

        for i in 0..clusters.len() {
            for j in (i + 1)..clusters.len() {
                let d =
                    cluster_single_distance(&clusters[i].members, &clusters[j].members, distance);
                if d < best.2 {
                    best = (i, j, d);
                }
            }
        }

        if !best.2.is_finite() {
            return Vec::new();
        }

        let (left_idx, right_idx, merge_distance) = best;
        let right = clusters.remove(right_idx);
        let left = clusters.remove(left_idx);
        let mut members = left.members;
        members.extend(right.members);
        let size = members.len();

        linkage.push(LinkageMerge {
            left: left.id,
            right: right.id,
            distance: merge_distance,
            size,
        });
        clusters.push(Cluster {
            id: next_id,
            members,
        });
        next_id += 1;
    }

    linkage
}

/// Reorder assets by recursively traversing the clustering tree.
///
/// This is the quasi-diagonalization step from López de Prado's HRP recipe:
/// nearby leaves in the dendrogram become nearby entries in the covariance
/// matrix ordering.
fn hrp_quasi_diagonalization(linkage: &[LinkageMerge], n_assets: usize) -> Vec<usize> {
    if n_assets == 0 {
        return Vec::new();
    }
    if n_assets == 1 {
        return vec![0];
    }
    if linkage.len() != n_assets - 1 {
        return Vec::new();
    }

    let root_id = n_assets + linkage.len() - 1;
    let mut order = Vec::with_capacity(n_assets);
    append_linkage_leaves(root_id, n_assets, linkage, &mut order);

    if order.len() == n_assets && all_unique_indices(&order, n_assets) {
        order
    } else {
        Vec::new()
    }
}

/// Allocate HRP weights by recursive bisection on quasi-diagonalized assets.
///
/// Cluster variance is estimated with the inverse-variance portfolio inside
/// each cluster. Returned weights are in original asset order.
fn hrp_recursive_bisection(covariance: &[Vec<f64>], ordered_indices: &[usize]) -> Vec<f64> {
    let n = covariance.len();
    if n == 0
        || covariance.iter().any(|row| row.len() != n)
        || ordered_indices.len() != n
        || !all_unique_indices(ordered_indices, n)
    {
        return Vec::new();
    }
    if n == 1 {
        return vec![1.0];
    }

    let mut ordered_weights = vec![1.0; n];
    let mut ranges = vec![(0_usize, n)];

    while let Some((start, end)) = ranges.pop() {
        let len = end - start;
        if len <= 1 {
            continue;
        }

        let mid = start + len / 2;
        let left = &ordered_indices[start..mid];
        let right = &ordered_indices[mid..end];
        let left_var = cluster_variance(covariance, left);
        let right_var = cluster_variance(covariance, right);
        let denom = left_var + right_var;
        let alpha = if denom.is_finite() && denom > 1e-18 {
            1.0 - left_var / denom
        } else {
            0.5
        };

        for weight in &mut ordered_weights[start..mid] {
            *weight *= alpha;
        }
        for weight in &mut ordered_weights[mid..end] {
            *weight *= 1.0 - alpha;
        }

        ranges.push((start, mid));
        ranges.push((mid, end));
    }

    let mut weights = vec![0.0; n];
    for (ordered_pos, asset_idx) in ordered_indices.iter().enumerate() {
        weights[*asset_idx] = ordered_weights[ordered_pos];
    }

    normalize_long_only(weights)
}

/// Hierarchical Risk Parity optimizer following López de Prado (2016).
///
/// The pipeline is: sample covariance/correlation, correlation distance,
/// single-linkage clustering, quasi-diagonalization, then recursive bisection.
/// Invalid input returns empty weights, matching the existing optimizer API.
pub fn optimize_hrp(returns: &[Vec<f64>]) -> Vec<f64> {
    let Some((_rows, cols)) = matrix_shape(returns) else {
        return Vec::new();
    };
    if cols == 1 {
        return vec![1.0];
    }

    let corr = correlation_matrix(returns);
    let dist = distance_matrix(&corr);
    let linkage = single_linkage_clustering(&dist);
    let ordered = hrp_quasi_diagonalization(&linkage, cols);
    if ordered.is_empty() {
        return Vec::new();
    }

    let cov = covariance_matrix(returns);
    let weights = hrp_recursive_bisection(&cov, &ordered);
    if weights.len() == cols && weights.iter().all(|w| w.is_finite() && *w >= -1e-12) {
        normalize_long_only(weights)
    } else {
        Vec::new()
    }
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

fn raw_covariance_matrix(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
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

    cov
}

fn covariance_matrix(matrix: &[Vec<f64>]) -> Vec<Vec<f64>> {
    let mut cov = raw_covariance_matrix(matrix);
    let cols = cov.len();

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

/// Euclidean projection onto the unit simplex `{ w ∈ ℝⁿ : wᵢ ≥ 0, Σwᵢ = 1 }`.
///
/// Implements Duchi et al. (2008), "Efficient Projections onto the
/// ℓ₁-Ball for Learning in High Dimensions".
///
/// # Errors
///
/// - [`OptimizeError::EmptyInput`] if `v` is empty.
/// - [`OptimizeError::DegenerateProjection`] if `v` has no positive
///   finite component. Such input is mathematically projectable (the
///   answer is `[1/n, …, 1/n]`), but returning equal-weights silently
///   masks upstream bugs — e.g., a gradient step that zeroed the
///   iterate or an input slice full of `NaN`. Surfacing the condition
///   as an error lets callers decide whether to restart, fall back, or
///   propagate.
pub fn project_simplex(v: &[f64]) -> Result<Vec<f64>, OptimizeError> {
    if v.is_empty() {
        return Err(OptimizeError::EmptyInput);
    }
    if !v.iter().any(|x| x.is_finite() && *x > 0.0) {
        return Err(OptimizeError::DegenerateProjection);
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

    // rho >= 1 is guaranteed when at least one positive finite component
    // exists: at i where u[i] is that positive value, u[i] - theta > 0
    // because the first partial sum can never exceed i+1.
    debug_assert!(rho > 0);
    let theta = (u[..rho].iter().sum::<f64>() - 1.0) / rho as f64;
    let projected: Vec<f64> = v.iter().map(|x| (x - theta).max(0.0)).collect();
    Ok(normalize_long_only(projected))
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

fn cluster_single_distance(left: &[usize], right: &[usize], distance: &[Vec<f64>]) -> f64 {
    let mut best = f64::INFINITY;
    for &i in left {
        for &j in right {
            let d = distance[i][j];
            if d.is_finite() && d < best {
                best = d;
            }
        }
    }
    best
}

fn append_linkage_leaves(
    cluster_id: usize,
    n_assets: usize,
    linkage: &[LinkageMerge],
    order: &mut Vec<usize>,
) {
    if cluster_id < n_assets {
        order.push(cluster_id);
        return;
    }

    let merge_idx = cluster_id - n_assets;
    if let Some(merge) = linkage.get(merge_idx) {
        append_linkage_leaves(merge.left, n_assets, linkage, order);
        append_linkage_leaves(merge.right, n_assets, linkage, order);
    }
}

fn all_unique_indices(indices: &[usize], n: usize) -> bool {
    let mut seen = vec![false; n];
    for &idx in indices {
        if idx >= n || seen[idx] {
            return false;
        }
        seen[idx] = true;
    }
    true
}

fn cluster_variance(covariance: &[Vec<f64>], indices: &[usize]) -> f64 {
    debug_assert!(!indices.is_empty());
    if indices.len() == 1 {
        let var = covariance[indices[0]][indices[0]];
        return if var.is_finite() && var > 0.0 {
            var
        } else {
            1e-12
        };
    }

    let diag_risks: Vec<f64> = indices
        .iter()
        .map(|&idx| {
            let var = covariance[idx][idx];
            if var.is_finite() && var > 1e-12 {
                var
            } else {
                1e-12
            }
        })
        .collect();
    let weights = inverse_risk_weights(&diag_risks);

    let mut variance = 0.0;
    for (local_i, &asset_i) in indices.iter().enumerate() {
        for (local_j, &asset_j) in indices.iter().enumerate() {
            variance += weights[local_i] * covariance[asset_i][asset_j] * weights[local_j];
        }
    }

    if variance.is_finite() && variance > 0.0 {
        variance
    } else {
        1e-12
    }
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
    fn correlation_matrix_is_symmetric_with_unit_diagonal() {
        let r = sample_returns();
        let corr = correlation_matrix(&r);

        assert_eq!(corr.len(), 3);
        for i in 0..3 {
            assert!((corr[i][i] - 1.0).abs() < 1e-12);
            for j in 0..3 {
                assert!((corr[i][j] - corr[j][i]).abs() < 1e-12);
                assert!(corr[i][j].is_finite());
                assert!((-1.0..=1.0).contains(&corr[i][j]));
            }
        }
    }

    #[test]
    fn distance_matrix_uses_correlation_distance() {
        let corr = vec![vec![1.0, 0.5], vec![0.5, 1.0]];
        let dist = distance_matrix(&corr);

        assert_eq!(dist.len(), 2);
        assert_eq!(dist[0][0], 0.0);
        assert_eq!(dist[1][1], 0.0);
        let expected = 0.5;
        assert!((dist[0][1] - expected).abs() < 1e-12);
        assert!((dist[1][0] - expected).abs() < 1e-12);
    }

    #[test]
    fn single_linkage_and_quasi_diagonalization_cluster_nearest_assets() {
        let dist = vec![
            vec![0.0, 0.1, 0.8],
            vec![0.1, 0.0, 0.7],
            vec![0.8, 0.7, 0.0],
        ];

        let linkage = single_linkage_clustering(&dist);
        assert_eq!(linkage.len(), 2);
        assert_eq!(linkage[0].left, 0);
        assert_eq!(linkage[0].right, 1);
        assert!((linkage[0].distance - 0.1).abs() < 1e-12);

        let order = hrp_quasi_diagonalization(&linkage, 3);
        assert_eq!(order, vec![2, 0, 1]);
    }

    #[test]
    fn hrp_weights_are_valid() {
        let r = sample_returns();
        let w = optimize_hrp(&r);
        assert_valid_weights(&w, 3);
    }

    #[test]
    fn hrp_perfectly_correlated_equal_variance_assets_get_equal_weights() {
        let factor = [-0.02, -0.01, 0.00, 0.01, 0.02];
        let returns: Vec<Vec<f64>> = factor.iter().map(|&x| vec![x, x, x, x]).collect();

        let w = optimize_hrp(&returns);

        assert_valid_weights(&w, 4);
        assert_close(&w, &[0.25, 0.25, 0.25, 0.25], 1e-12);
    }

    #[test]
    fn hrp_uncorrelated_equal_variance_assets_get_equal_weights() {
        let returns = vec![
            vec![1.0, 0.0, 1.0, 0.0],
            vec![0.0, 1.0, 0.0, 1.0],
            vec![-1.0, 0.0, -1.0, 0.0],
            vec![0.0, -1.0, 0.0, -1.0],
        ];

        let w = optimize_hrp(&returns);

        assert_valid_weights(&w, 4);
        assert_close(&w, &[0.25, 0.25, 0.25, 0.25], 1e-6);
    }

    #[test]
    fn hrp_two_asset_case_matches_inverse_variance_solution() {
        let returns = vec![
            vec![-2.0, -1.0],
            vec![2.0, 1.0],
            vec![-2.0, -1.0],
            vec![2.0, 1.0],
        ];

        let w = optimize_hrp(&returns);

        assert_valid_weights(&w, 2);
        assert_close(&w, &[0.2, 0.8], 1e-6);
    }

    #[test]
    fn hrp_edge_cases_follow_optimizer_contract() {
        assert!(optimize_hrp(&[]).is_empty());
        assert!(optimize_hrp(&[vec![0.01, 0.02], vec![0.03]]).is_empty());
        assert_eq!(optimize_hrp(&[vec![0.01], vec![-0.02]]), vec![1.0]);
        assert!(correlation_matrix(&[vec![0.01, f64::NAN], vec![0.02, 0.03]]).is_empty());
    }

    #[test]
    fn hrp_reference_test_with_known_clustering_structure() {
        // 3-asset case with clear correlation structure:
        // Asset 0 and 1 are highly correlated, asset 2 is uncorrelated with both
        let returns = vec![
            vec![0.01, 0.009, 0.0],
            vec![-0.01, -0.011, 0.0],
            vec![0.02, 0.019, 0.0],
            vec![-0.02, -0.021, 0.0],
            vec![0.015, 0.014, 0.0],
            vec![-0.015, -0.016, 0.0],
        ];

        let corr = correlation_matrix(&returns);

        // Verify correlation structure: assets 0 and 1 highly correlated, asset 2 uncorrelated
        assert!(
            corr[0][1] > 0.99,
            "Assets 0 and 1 should be highly correlated"
        );
        assert!(
            corr[0][2].abs() < 0.1,
            "Asset 2 should be uncorrelated with asset 0"
        );
        assert!(
            corr[1][2].abs() < 0.1,
            "Asset 2 should be uncorrelated with asset 1"
        );

        // HRP should cluster assets 0 and 1 together, separate from asset 2
        let w = optimize_hrp(&returns);

        assert_valid_weights(&w, 3);

        // Assets 0 and 1 should get similar weights (clustered together)
        // Asset 2 should get a different weight (separate cluster)
        let w0 = w[0];
        let w1 = w[1];
        let w2 = w[2];

        let cluster_01_diff = (w0 - w1).abs();
        let cluster_01_vs_2_diff = ((w0 + w1) / 2.0 - w2).abs();

        assert!(
            cluster_01_diff < cluster_01_vs_2_diff,
            "Assets 0 and 1 should have more similar weights than either vs asset 2"
        );
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

    // ========================================================================
    // N18: convergence diagnostics
    // ========================================================================

    #[test]
    fn min_variance_ex_reports_converged_when_tol_triggers() {
        // Generous tolerance: the very first step's squared distance
        // will already be below it, so convergence fires on iter 1.
        let r = sample_returns();
        let opts = OptimizerOptions {
            max_iters: 350,
            tol: 1.0, // absurdly loose
        };
        let res = optimize_min_variance_ex(&r, opts);
        assert!(
            res.converged,
            "should converge immediately under a huge tol"
        );
        assert_eq!(res.iters, 1);
        assert_valid_weights(&res.weights, 3);
    }

    #[test]
    fn min_variance_ex_reports_not_converged_when_budget_exhausted() {
        // Tight tolerance: step size decays but never reaches 0 — the
        // loop must burn the whole budget.
        let r = sample_returns();
        let opts = OptimizerOptions {
            max_iters: 50,
            tol: 0.0, // unreachable for projected gradient on the simplex
        };
        let res = optimize_min_variance_ex(&r, opts);
        assert!(!res.converged, "tol=0 is unreachable; must exhaust budget");
        assert_eq!(res.iters, 50);
        assert!(res.final_step_squared.is_finite());
        assert_valid_weights(&res.weights, 3);
    }

    #[test]
    fn min_variance_wrapper_equals_ex_default() {
        let r = sample_returns();
        let w = optimize_min_variance(&r);
        let res = optimize_min_variance_ex(&r, OptimizerOptions::default());
        assert_eq!(w, res.weights, "wrapper must equal _ex with defaults");
    }

    #[test]
    fn min_variance_ex_empty_input_returns_empty() {
        let res = optimize_min_variance_ex(&[], OptimizerOptions::default());
        assert!(res.weights.is_empty());
        assert_eq!(res.iters, 0);
        assert!(!res.converged);
    }

    /// Documents the plan's observation: at the default `tol = 1e-16`
    /// on daily-return-scale data, the optimizer never early-stops and
    /// exhausts the 350-iter budget. This test pins that behaviour so a
    /// future convergence improvement is explicit rather than silent.
    #[test]
    fn min_variance_ex_exhausts_default_budget_on_sample_returns() {
        let r = sample_returns();
        let res = optimize_min_variance_ex(&r, OptimizerOptions::default());
        assert!(
            !res.converged,
            "1e-16 tol shouldn't trigger on daily-scale data"
        );
        assert_eq!(res.iters, 350);
    }

    #[test]
    fn min_variance_ex_single_asset_is_trivially_converged() {
        let r = vec![vec![0.01], vec![-0.005], vec![0.02]];
        let res = optimize_min_variance_ex(&r, OptimizerOptions::default());
        assert_eq!(res.weights, vec![1.0]);
        assert!(res.converged);
        assert_eq!(res.iters, 0);
    }

    /// N17 acceptance: degenerate input surfaces as an explicit error
    /// rather than a silent equal-weight fallback.
    #[test]
    fn project_simplex_on_all_zeros_errors() {
        assert!(matches!(
            project_simplex(&[0.0, 0.0, 0.0]),
            Err(OptimizeError::DegenerateProjection)
        ));
    }

    #[test]
    fn project_simplex_on_empty_errors() {
        assert!(matches!(
            project_simplex(&[]),
            Err(OptimizeError::EmptyInput)
        ));
    }

    #[test]
    fn project_simplex_on_all_negative_errors() {
        assert!(matches!(
            project_simplex(&[-1.0, -0.5, -2.3]),
            Err(OptimizeError::DegenerateProjection)
        ));
    }

    #[test]
    fn project_simplex_on_all_nan_errors() {
        assert!(matches!(
            project_simplex(&[f64::NAN, f64::NAN]),
            Err(OptimizeError::DegenerateProjection)
        ));
    }

    #[test]
    fn project_simplex_valid_input_projects() {
        // A point outside the simplex; Euclidean projection hits the
        // simplex point closest to it.
        let w = project_simplex(&[0.5, 0.5, 0.5]).unwrap();
        let s: f64 = w.iter().sum();
        assert!((s - 1.0).abs() < 1e-12, "sum={s}");
        assert!(w.iter().all(|x| *x >= -1e-12));
    }

    #[test]
    fn project_simplex_on_simplex_point_is_idempotent() {
        let p = vec![0.25, 0.50, 0.25];
        let w = project_simplex(&p).unwrap();
        for (a, b) in w.iter().zip(p.iter()) {
            assert!((a - b).abs() < 1e-12);
        }
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
