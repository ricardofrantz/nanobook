//! One-step-ahead variance forecasting using an EWMA-style recursion.
//!
//! Despite the historical name `garch_forecast`, this is NOT a maximum-
//! likelihood-fitted GARCH(p,q). The recursion uses fixed parameters
//! alpha = 0.08, beta = 0.90, and omega derived from the sample variance:
//!
//! ```text
//! h[t+1] = omega + sum(alpha_j * eps[t+1-j]^2)
//!          + sum(beta_k * h[t+1-k])
//! ```
//!
//! For MLE-fitted GARCH, use the Python `arch` package or wait for the
//! `garch-mle` feature flag in nanobook >= 0.11.

/// One-step-ahead volatility forecast with fixed EWMA-style parameters.
///
/// Returns per-period standard deviation (not variance, not annualized).
///
/// Behavior:
/// - Invalid/non-finite inputs fall back to sample volatility.
/// - `mean` supports `"zero"` and `"constant"`/`"mean"`.
/// - `p`/`q` are clamped to a small bounded range for numerical stability.
pub fn garch_ewma_forecast(returns: &[f64], p: usize, q: usize, mean: &str) -> f64 {
    let fallback = sample_volatility(returns);

    if returns.len() < 2 || returns.iter().any(|r| !r.is_finite()) {
        return fallback;
    }

    let p = p.clamp(1, 8).min(returns.len());
    let q = q.clamp(1, 8).min(returns.len());

    let use_constant_mean = matches!(mean.to_ascii_lowercase().as_str(), "constant" | "mean");
    let mu = if use_constant_mean {
        returns.iter().sum::<f64>() / returns.len() as f64
    } else {
        0.0
    };

    let eps: Vec<f64> = returns.iter().map(|r| r - mu).collect();
    let var0 = sample_variance(&eps).unwrap_or(0.0).max(1e-12);
    if !var0.is_finite() || var0 <= 0.0 {
        return fallback;
    }

    // Conservative coefficient totals ensure stationarity.
    let total_alpha = 0.08_f64;
    let total_beta = 0.90_f64;

    let alphas = decaying_weights(p, total_alpha, 0.80);
    let betas = decaying_weights(q, total_beta, 0.85);

    let alpha_sum = alphas.iter().sum::<f64>();
    let beta_sum = betas.iter().sum::<f64>();
    let omega = (1.0 - alpha_sum - beta_sum).max(1e-6) * var0;

    // Conditional variance history h_t. h[0] is initialization.
    let mut h = vec![var0; eps.len() + 1];

    for t in 1..=eps.len() {
        let mut arch_term = 0.0;
        for i in 1..=p {
            if t >= i {
                let e = eps[t - i];
                arch_term += alphas[i - 1] * e * e;
            }
        }

        let mut garch_term = 0.0;
        for j in 1..=q {
            if t >= j {
                garch_term += betas[j - 1] * h[t - j];
            }
        }

        h[t] = (omega + arch_term + garch_term).max(1e-12);
    }

    // One-step-ahead forecast h_{T+1}
    let t = eps.len();
    let mut arch_next = 0.0;
    for i in 1..=p {
        if t >= i {
            let e = eps[t - i];
            arch_next += alphas[i - 1] * e * e;
        }
    }

    let mut garch_next = 0.0;
    debug_assert!(t + 1 >= q, "caller must pass at least q returns");
    for j in 1..=q {
        garch_next += betas[j - 1] * h[t + 1 - j];
    }

    let sigma = (omega + arch_next + garch_next).max(1e-12).sqrt();
    if sigma.is_finite() && sigma >= 0.0 {
        sigma
    } else {
        fallback
    }
}

/// Deprecated alias for [`garch_ewma_forecast`]; removed in v0.11.
#[rustfmt::skip]
#[deprecated(since = "0.9.3", note = "use `garch_ewma_forecast`; parameters are fixed, not MLE-fit")]
pub fn garch_forecast(returns: &[f64], p: usize, q: usize, mean: &str) -> f64 {
    garch_ewma_forecast(returns, p, q, mean)
}

fn sample_volatility(returns: &[f64]) -> f64 {
    sample_variance(returns).unwrap_or(0.0).max(0.0).sqrt()
}

fn sample_variance(values: &[f64]) -> Option<f64> {
    let n = values.len();
    if n < 2 {
        return None;
    }

    let mean = values.iter().sum::<f64>() / n as f64;
    let ss = values
        .iter()
        .map(|v| {
            let d = v - mean;
            d * d
        })
        .sum::<f64>();

    let var = ss / (n as f64 - 1.0);
    if var.is_finite() { Some(var) } else { None }
}

fn decaying_weights(count: usize, total: f64, decay: f64) -> Vec<f64> {
    if count == 0 || total <= 0.0 {
        return Vec::new();
    }

    let mut raw = Vec::with_capacity(count);
    let mut x = 1.0;
    for _ in 0..count {
        raw.push(x);
        x *= decay;
    }

    let denom = raw.iter().sum::<f64>().max(1e-12);
    raw.into_iter().map(|w| w / denom * total).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ewma_forecast_is_finite_on_valid_input() {
        let returns = vec![0.01, -0.004, 0.008, -0.002, 0.005, -0.003, 0.004];
        let v = garch_ewma_forecast(&returns, 1, 1, "zero");
        assert!(v.is_finite());
        assert!(v >= 0.0);
    }

    #[test]
    fn ewma_forecast_handles_constant_mean_mode() {
        let returns = vec![0.02, 0.01, 0.015, 0.0, -0.005, 0.01, 0.012];
        let v = garch_ewma_forecast(&returns, 2, 1, "constant");
        assert!(v.is_finite());
        assert!(v >= 0.0);
    }

    #[test]
    fn invalid_input_falls_back() {
        let returns = vec![0.01, f64::NAN, 0.02];
        let v = garch_ewma_forecast(&returns, 1, 1, "zero");
        assert!(v.is_finite());
        assert!(v >= 0.0);
    }

    #[test]
    fn ewma_forecast_short_input_is_bounded() {
        let returns = vec![0.01];
        let v = garch_ewma_forecast(&returns, 1, 4, "zero");
        assert!(v.is_finite());
        assert!(v >= 0.0);
    }

    #[allow(deprecated)]
    #[test]
    fn deprecated_garch_forecast_shim_delegates() {
        let returns = vec![0.01, -0.004, 0.008, -0.002, 0.005, -0.003, 0.004];
        assert_eq!(
            garch_forecast(&returns, 2, 2, "constant"),
            garch_ewma_forecast(&returns, 2, 2, "constant")
        );
    }

    #[test]
    fn qtrade_reference_fixture_targets() {
        // Fixed fixture used by qtrade v0.4 bridge parity checks.
        let returns = vec![
            0.011, -0.007, 0.004, -0.002, 0.006, -0.003, 0.002, 0.001, -0.004, 0.005, -0.001, 0.003,
        ];

        let zero = garch_ewma_forecast(&returns, 1, 1, "zero");
        let constant = garch_ewma_forecast(&returns, 2, 1, "constant");

        assert!((zero - 0.0044776400483411).abs() < 5e-14, "zero={zero}");
        assert!(
            (constant - 0.0043960525154678).abs() < 5e-14,
            "constant={constant}"
        );
    }
}
