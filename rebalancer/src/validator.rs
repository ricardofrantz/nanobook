//! Startup validation helpers for production-safe rebalancer runs.
//!
//! These checks are intentionally side-effect-light: they validate static
//! configuration, writable paths, timeout ranges, and provide a hook for broker
//! connectivity probes without requiring the trading workflow to start.

use std::fmt::Write as _;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::path::Path;
use std::time::Duration;

use crate::config::Config;
use crate::error::{Error, Result};

const MIN_FREE_BYTES_FOR_LOGS: u64 = 1_000_000_000;

/// A single actionable validation failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationIssue {
    pub field: String,
    pub message: String,
    pub suggestion: String,
}

impl ValidationIssue {
    fn new(
        field: impl Into<String>,
        message: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            field: field.into(),
            message: message.into(),
            suggestion: suggestion.into(),
        }
    }
}

/// Validate all static startup checks that do not require broker credentials.
pub fn validate_static(config: &Config) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    issues.extend(validate_required_fields(config));
    issues.extend(validate_risk_limits(config));
    issues.extend(validate_network_timeout(config));
    issues.extend(validate_file_permissions(config));
    issues.extend(validate_disk_space(config));
    issues
}

/// Fail if any static validation issue is present.
pub fn should_run_startup_validation(skip_validation: bool) -> bool {
    !skip_validation
}

pub fn validate_static_or_error(config: &Config) -> Result<()> {
    let issues = validate_static(config);
    if issues.is_empty() {
        return Ok(());
    }

    Err(Error::Config(format_validation_issues(&issues)))
}

/// Format validation issues for operator-facing output.
pub fn format_validation_issues(issues: &[ValidationIssue]) -> String {
    let mut out = String::from("startup validation failed:");
    for issue in issues {
        let _ = write!(
            out,
            "\n  - {}: {}\n    fix: {}",
            issue.field, issue.message, issue.suggestion
        );
    }
    out
}

pub fn validate_required_fields(config: &Config) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if config.connection.host.trim().is_empty() {
        issues.push(ValidationIssue::new(
            "connection.host",
            "host must not be empty",
            "set connection.host to the IBKR Gateway/TWS host, e.g. 127.0.0.1",
        ));
    }
    if config.account.id.trim().is_empty() {
        issues.push(ValidationIssue::new(
            "account.id",
            "account id must not be empty",
            "set account.id to the IBKR account identifier",
        ));
    }
    if config.logging.dir.trim().is_empty() {
        issues.push(ValidationIssue::new(
            "logging.dir",
            "logging directory must not be empty",
            "set logging.dir to a writable directory for audit logs",
        ));
    }
    issues
}

pub fn validate_risk_limits(config: &Config) -> Vec<ValidationIssue> {
    let mut issues = Vec::new();
    if !(config.risk.max_position_pct > 0.0 && config.risk.max_position_pct <= 1.0) {
        issues.push(ValidationIssue::new(
            "risk.max_position_pct",
            format!("{} is outside (0.0, 1.0]", config.risk.max_position_pct),
            "choose a fractional max position such as 0.25 for 25%",
        ));
    }
    if !(1.0..=10.0).contains(&config.risk.max_leverage) {
        issues.push(ValidationIssue::new(
            "risk.max_leverage",
            format!("{} is outside [1.0, 10.0]", config.risk.max_leverage),
            "choose a leverage cap between 1.0 and 10.0",
        ));
    }
    if config.risk.min_trade_usd < 0.0 {
        issues.push(ValidationIssue::new(
            "risk.min_trade_usd",
            format!("{} is negative", config.risk.min_trade_usd),
            "use 0.0 or a positive minimum trade size",
        ));
    }
    if config.risk.max_trade_usd <= 0.0 {
        issues.push(ValidationIssue::new(
            "risk.max_trade_usd",
            format!("{} must be positive", config.risk.max_trade_usd),
            "set a positive maximum trade size",
        ));
    }
    issues
}

pub fn validate_network_timeout(config: &Config) -> Vec<ValidationIssue> {
    let timeout = config.connection.timeout_secs;
    if !(5..=60).contains(&timeout) {
        return vec![ValidationIssue::new(
            "connection.timeout_secs",
            format!("{timeout}s is outside [5, 60] seconds"),
            "set connection.timeout_secs between 5 and 60 seconds",
        )];
    }
    Vec::new()
}

pub fn validate_file_permissions(config: &Config) -> Vec<ValidationIssue> {
    let audit_path = config.audit_path();
    let Some(dir) = audit_path.parent() else {
        return vec![ValidationIssue::new(
            "logging.dir",
            "audit log path has no parent directory",
            "set logging.dir to a writable directory",
        )];
    };

    if let Err(error) = std::fs::create_dir_all(dir) {
        return vec![ValidationIssue::new(
            "logging.dir",
            format!(
                "cannot create audit log directory {}: {error}",
                dir.display()
            ),
            "create the directory manually or fix directory permissions",
        )];
    }

    let probe = dir.join(".nanobook-write-probe");
    match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&probe)
    {
        Ok(_) => {
            let _ = std::fs::remove_file(probe);
            Vec::new()
        }
        Err(error) => vec![ValidationIssue::new(
            "logging.dir",
            format!("directory {} is not writable: {error}", dir.display()),
            "grant write permission to the rebalancer user",
        )],
    }
}

pub fn validate_disk_space(config: &Config) -> Vec<ValidationIssue> {
    let audit_path = config.audit_path();
    let dir = audit_path.parent().unwrap_or_else(|| Path::new("."));
    match fs2::available_space(dir) {
        Ok(bytes) => validate_available_log_space(bytes, config),
        Err(error) => vec![ValidationIssue::new(
            "logging.dir",
            format!(
                "cannot determine available space for {}: {error}",
                dir.display()
            ),
            "verify the logging directory exists and the filesystem is accessible",
        )],
    }
}

pub fn validate_available_log_space(
    available_bytes: u64,
    _config: &Config,
) -> Vec<ValidationIssue> {
    if available_bytes < MIN_FREE_BYTES_FOR_LOGS {
        return vec![ValidationIssue::new(
            "logging.dir",
            format!("only {available_bytes} bytes available for logs"),
            "free at least 1GB for audit/general logs before starting",
        )];
    }
    Vec::new()
}

/// Best-effort TCP connectivity check for the configured broker endpoint.
pub fn validate_broker_connectivity(config: &Config) -> Vec<ValidationIssue> {
    let address = config.address();
    let timeout = Duration::from_secs(config.connection.timeout_secs.clamp(1, 60));
    let Ok(mut addrs) = address.to_socket_addrs() else {
        return vec![ValidationIssue::new(
            "connection.host",
            format!("cannot resolve broker address {address}"),
            "check connection.host and connection.port",
        )];
    };
    let Some(addr) = addrs.next() else {
        return vec![ValidationIssue::new(
            "connection.host",
            format!("broker address {address} resolved to no socket addresses"),
            "check connection.host and connection.port",
        )];
    };
    match TcpStream::connect_timeout(&addr, timeout) {
        Ok(stream) => {
            let _ = stream.shutdown(Shutdown::Both);
            Vec::new()
        }
        Err(error) => vec![ValidationIssue::new(
            "connection",
            format!("cannot connect to broker at {address}: {error}"),
            "start IBKR Gateway/TWS, confirm API is enabled, and verify host/port",
        )],
    }
}

#[allow(dead_code)]
fn _path_exists(path: &Path) -> bool {
    path.exists()
}

#[cfg(test)]
mod tests {
    use super::{
        ValidationIssue, format_validation_issues, should_run_startup_validation,
        validate_available_log_space, validate_file_permissions, validate_network_timeout,
        validate_risk_limits, validate_static,
    };
    use crate::config::{
        AccountConfig, AccountType, Config, ConnectionConfig, CostConfig, ExecutionConfig,
        LoggingConfig, RiskConfig,
    };
    use std::path::Path;

    fn valid_config(dir: &Path) -> Config {
        Config {
            connection: ConnectionConfig {
                host: "127.0.0.1".into(),
                port: 4002,
                client_id: 1,
                timeout_secs: 30,
            },
            account: AccountConfig {
                id: "DU123".into(),
                account_type: AccountType::Margin,
            },
            execution: ExecutionConfig {
                order_interval_ms: 100,
                limit_offset_bps: 5,
                order_timeout_secs: 300,
                max_orders_per_run: 50,
                quote_staleness_threshold_sec: 30,
            },
            risk: RiskConfig::default(),
            cost: CostConfig {
                commission_per_share: 0.0035,
                commission_min: 0.35,
                slippage_bps: 5,
            },
            logging: LoggingConfig {
                dir: dir.display().to_string(),
                audit_file: "audit.jsonl".into(),
                clock_skew_threshold_sec: 300,
                max_jump_rate_sec_per_sec: 2.0,
            },
        }
    }

    #[test]
    fn static_validation_accepts_valid_config() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let config = valid_config(dir.path());
        assert!(validate_static(&config).is_empty());
        Ok(())
    }

    #[test]
    fn risk_limits_reject_out_of_range_values() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let mut config = valid_config(dir.path());
        config.risk.max_position_pct = 1.5;
        config.risk.max_leverage = 20.0;
        let issues = validate_risk_limits(&config);
        assert!(issues.iter().any(|i| i.field == "risk.max_position_pct"));
        assert!(issues.iter().any(|i| i.field == "risk.max_leverage"));
        Ok(())
    }

    #[test]
    fn timeout_validation_rejects_too_short_and_too_long() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let mut config = valid_config(dir.path());
        config.connection.timeout_secs = 1;
        assert_eq!(
            validate_network_timeout(&config)[0].field,
            "connection.timeout_secs"
        );
        config.connection.timeout_secs = 120;
        assert_eq!(
            validate_network_timeout(&config)[0].field,
            "connection.timeout_secs"
        );
        Ok(())
    }

    #[test]
    fn disk_space_validation_rejects_low_space() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let config = valid_config(dir.path());
        let issues = validate_available_log_space(512, &config);
        assert_eq!(issues[0].field, "logging.dir");
        assert!(issues[0].suggestion.contains("1GB"));
        Ok(())
    }

    #[test]
    fn file_permissions_create_writable_log_dir() -> std::io::Result<()> {
        let dir = tempfile::tempdir()?;
        let nested = dir.path().join("logs");
        let config = valid_config(&nested);
        assert!(validate_file_permissions(&config).is_empty());
        assert!(nested.exists());
        Ok(())
    }

    #[test]
    fn skip_validation_flag_controls_startup_validation() {
        assert!(should_run_startup_validation(false));
        assert!(!should_run_startup_validation(true));
    }

    #[test]
    fn formatted_validation_issues_are_actionable() {
        let issue = ValidationIssue::new("risk.max_leverage", "20 is too high", "use <= 10");
        let text = format_validation_issues(&[issue]);
        assert!(text.contains("risk.max_leverage"));
        assert!(text.contains("20 is too high"));
        assert!(text.contains("fix: use <= 10"));
    }
}
