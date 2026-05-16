//! OHLC realized volatility estimators.

/// Realized-volatility estimator method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealizedVolMethod {
    CloseToClose,
    Parkinson,
    GarmanKlass,
    YangZhang,
}

impl RealizedVolMethod {
    pub fn parse(method: &str) -> Option<Self> {
        match method {
            "close_to_close" | "close" | "cc" => Some(Self::CloseToClose),
            "parkinson" => Some(Self::Parkinson),
            "garman_klass" | "garman-klass" | "gk" => Some(Self::GarmanKlass),
            "yang_zhang" | "yang-zhang" | "yz" => Some(Self::YangZhang),
            _ => None,
        }
    }
}

/// Estimate per-period realized volatility from OHLC bars.
pub fn realized_vol(
    open: &[f64],
    high: &[f64],
    low: &[f64],
    close: &[f64],
    method: RealizedVolMethod,
) -> f64 {
    if open.len() != high.len()
        || open.len() != low.len()
        || open.len() != close.len()
        || close.len() < 2
    {
        return f64::NAN;
    }
    if open
        .iter()
        .chain(high)
        .chain(low)
        .chain(close)
        .any(|v| !v.is_finite() || *v <= 0.0)
    {
        return f64::NAN;
    }

    match method {
        RealizedVolMethod::CloseToClose => close_to_close(close),
        RealizedVolMethod::Parkinson => parkinson(high, low),
        RealizedVolMethod::GarmanKlass => garman_klass(open, high, low, close),
        RealizedVolMethod::YangZhang => yang_zhang(open, high, low, close),
    }
}

fn close_to_close(close: &[f64]) -> f64 {
    let returns: Vec<f64> = close.windows(2).map(|w| (w[1] / w[0]).ln()).collect();
    sample_variance(&returns).unwrap_or(0.0).max(0.0).sqrt()
}

fn parkinson(high: &[f64], low: &[f64]) -> f64 {
    let n = high.len() as f64;
    let sum: f64 = high
        .iter()
        .zip(low)
        .map(|(h, l)| (h / l).ln().powi(2))
        .sum();
    (sum / (4.0 * n * std::f64::consts::LN_2)).max(0.0).sqrt()
}

fn garman_klass(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> f64 {
    let n = open.len() as f64;
    let k = 2.0 * std::f64::consts::LN_2 - 1.0;
    let var: f64 = open
        .iter()
        .zip(high)
        .zip(low)
        .zip(close)
        .map(|(((o, h), l), c)| 0.5 * (h / l).ln().powi(2) - k * (c / o).ln().powi(2))
        .sum::<f64>()
        / n;
    var.max(0.0).sqrt()
}

fn yang_zhang(open: &[f64], high: &[f64], low: &[f64], close: &[f64]) -> f64 {
    let n = open.len();
    if n < 3 {
        return f64::NAN;
    }

    let overnight: Vec<f64> = (1..n).map(|i| (open[i] / close[i - 1]).ln()).collect();
    let open_close: Vec<f64> = (1..n).map(|i| (close[i] / open[i]).ln()).collect();
    let rs: Vec<f64> = (1..n)
        .map(|i| {
            let ho = (high[i] / open[i]).ln();
            let hc = (high[i] / close[i]).ln();
            let lo = (low[i] / open[i]).ln();
            let lc = (low[i] / close[i]).ln();
            ho * hc + lo * lc
        })
        .collect();

    let n_f = overnight.len() as f64;
    let k = 0.34 / (1.34 + (n_f + 1.0) / (n_f - 1.0));
    let var = sample_variance(&overnight).unwrap_or(0.0)
        + k * sample_variance(&open_close).unwrap_or(0.0)
        + (1.0 - k) * (rs.iter().sum::<f64>() / n_f);
    var.max(0.0).sqrt()
}

fn sample_variance(values: &[f64]) -> Option<f64> {
    if values.len() < 2 {
        return None;
    }
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    Some(values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / (values.len() - 1) as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ohlc() -> (Vec<f64>, Vec<f64>, Vec<f64>, Vec<f64>) {
        (
            vec![100.0, 102.0, 101.0, 105.0],
            vec![103.0, 104.0, 106.0, 108.0],
            vec![99.0, 100.0, 100.0, 104.0],
            vec![102.0, 101.0, 105.0, 107.0],
        )
    }

    #[test]
    fn estimators_return_finite_nonnegative_values() {
        let (o, h, l, c) = ohlc();
        for method in [
            RealizedVolMethod::CloseToClose,
            RealizedVolMethod::Parkinson,
            RealizedVolMethod::GarmanKlass,
            RealizedVolMethod::YangZhang,
        ] {
            let vol = realized_vol(&o, &h, &l, &c, method);
            assert!(vol.is_finite());
            assert!(vol >= 0.0);
        }
    }

    #[test]
    fn zero_range_estimators_are_zero_when_prices_constant() {
        let v = vec![100.0; 4];
        assert_eq!(
            realized_vol(&v, &v, &v, &v, RealizedVolMethod::Parkinson),
            0.0
        );
        assert_eq!(
            realized_vol(&v, &v, &v, &v, RealizedVolMethod::GarmanKlass),
            0.0
        );
    }

    #[test]
    fn invalid_inputs_return_nan() {
        assert!(
            realized_vol(
                &[1.0],
                &[1.0],
                &[1.0],
                &[1.0],
                RealizedVolMethod::CloseToClose
            )
            .is_nan()
        );
        assert!(RealizedVolMethod::parse("unknown").is_none());
    }
}
