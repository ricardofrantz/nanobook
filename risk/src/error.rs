//! Errors returned by the risk engine.

use std::fmt;

/// Errors returned by the risk engine.
///
/// Currently a single variant for configuration-validation failures;
/// extending this enum is non-breaking provided it stays
/// `#[non_exhaustive]`-friendly in spirit (match arms on library
/// callers should include a catch-all).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RiskError {
    /// A `RiskConfig` failed [`RiskConfig::validate`] — malformed
    /// numeric bounds (NaN, out-of-range, negative where non-negative
    /// required) or a config produced by deserializing untrusted
    /// input. The wrapped string is the failing-field message from
    /// `validate`, suitable for direct display or log output.
    ///
    /// [`RiskConfig::validate`]: crate::config::RiskConfig::validate
    InvalidConfig(String),
}

impl fmt::Display for RiskError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RiskError::InvalidConfig(msg) => write!(f, "invalid RiskConfig: {msg}"),
        }
    }
}

impl std::error::Error for RiskError {}
