//! Risk check report types.

use serde::Serialize;

/// Result of running all risk checks.
#[derive(Debug, Clone, Serialize)]
pub struct RiskReport {
    pub checks: Vec<RiskCheck>,
}

/// A single risk check result.
#[derive(Debug, Clone, Serialize)]
pub struct RiskCheck {
    pub name: &'static str,
    pub status: RiskStatus,
    pub detail: String,
}

/// Whether a check passed, warned, or failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum RiskStatus {
    Pass,
    Warn,
    Fail,
}

impl std::fmt::Display for RiskStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RiskStatus::Pass => write!(f, "PASS"),
            RiskStatus::Warn => write!(f, "WARN"),
            RiskStatus::Fail => write!(f, "FAIL"),
        }
    }
}

impl RiskReport {
    /// True if any check failed.
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|c| c.status == RiskStatus::Fail)
    }

    /// True if any check warned.
    pub fn has_warnings(&self) -> bool {
        self.checks.iter().any(|c| c.status == RiskStatus::Warn)
    }
}

impl std::fmt::Display for RiskReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "RISK CHECKS:")?;
        for check in &self.checks {
            writeln!(f, "  [{}] {}: {}", check.status, check.name, check.detail)?;
        }
        Ok(())
    }
}
