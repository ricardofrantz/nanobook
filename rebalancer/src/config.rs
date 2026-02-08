//! TOML configuration loading and validation.

use std::path::Path;

use serde::Deserialize;

use crate::error::{Error, Result};

/// Top-level configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct Config {
    pub connection: ConnectionConfig,
    pub account: AccountConfig,
    pub execution: ExecutionConfig,
    pub risk: RiskConfig,
    pub cost: CostConfig,
    pub logging: LoggingConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionConfig {
    pub host: String,
    pub port: u16,
    pub client_id: i32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u64,
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, Clone, Deserialize)]
pub struct AccountConfig {
    pub id: String,
    #[serde(rename = "type")]
    pub account_type: AccountType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AccountType {
    Cash,
    Margin,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default = "default_interval")]
    pub order_interval_ms: u64,
    #[serde(default = "default_offset")]
    pub limit_offset_bps: u32,
    #[serde(default = "default_order_timeout")]
    pub order_timeout_secs: u64,
    #[serde(default = "default_max_orders")]
    pub max_orders_per_run: usize,
}

fn default_interval() -> u64 {
    100
}
fn default_offset() -> u32 {
    5
}
fn default_order_timeout() -> u64 {
    300
}
fn default_max_orders() -> usize {
    50
}

#[derive(Debug, Clone, Deserialize)]
pub struct RiskConfig {
    #[serde(default = "default_max_position")]
    pub max_position_pct: f64,
    #[serde(default = "default_max_leverage")]
    pub max_leverage: f64,
    #[serde(default = "default_min_trade")]
    pub min_trade_usd: f64,
    #[serde(default = "default_max_trade")]
    pub max_trade_usd: f64,
    #[serde(default = "default_true")]
    pub allow_short: bool,
    #[serde(default = "default_max_short")]
    pub max_short_pct: f64,
}

fn default_max_position() -> f64 {
    0.25
}
fn default_max_leverage() -> f64 {
    1.5
}
fn default_min_trade() -> f64 {
    100.0
}
fn default_max_trade() -> f64 {
    100_000.0
}
fn default_true() -> bool {
    true
}
fn default_max_short() -> f64 {
    0.30
}

#[derive(Debug, Clone, Deserialize)]
pub struct CostConfig {
    #[serde(default = "default_commission")]
    pub commission_per_share: f64,
    #[serde(default = "default_commission_min")]
    pub commission_min: f64,
    #[serde(default = "default_slippage")]
    pub slippage_bps: u32,
}

fn default_commission() -> f64 {
    0.0035
}
fn default_commission_min() -> f64 {
    0.35
}
fn default_slippage() -> u32 {
    5
}

#[derive(Debug, Clone, Deserialize)]
pub struct LoggingConfig {
    #[serde(default = "default_log_dir")]
    pub dir: String,
    #[serde(default = "default_audit_file")]
    pub audit_file: String,
}

fn default_log_dir() -> String {
    "./logs".into()
}
fn default_audit_file() -> String {
    "audit.jsonl".into()
}

impl Config {
    /// Load config from a TOML file.
    pub fn load(path: &Path) -> Result<Self> {
        let contents = std::fs::read_to_string(path).map_err(|e| Error::ConfigRead {
            path: path.to_path_buf(),
            source: e,
        })?;
        let config: Config = toml::from_str(&contents)?;
        config.validate()?;
        Ok(config)
    }

    /// Validate config invariants.
    fn validate(&self) -> Result<()> {
        if self.connection.port == 0 {
            return Err(Error::Config("port must be > 0".into()));
        }
        if self.account.id.is_empty() {
            return Err(Error::Config("account id must not be empty".into()));
        }
        if self.risk.max_position_pct <= 0.0 || self.risk.max_position_pct > 1.0 {
            return Err(Error::Config(
                "max_position_pct must be in (0.0, 1.0]".into(),
            ));
        }
        if self.risk.max_leverage < 1.0 {
            return Err(Error::Config("max_leverage must be >= 1.0".into()));
        }
        if self.risk.min_trade_usd < 0.0 {
            return Err(Error::Config("min_trade_usd must be >= 0".into()));
        }
        if self.risk.max_trade_usd <= 0.0 {
            return Err(Error::Config("max_trade_usd must be > 0".into()));
        }
        if self.risk.max_short_pct < 0.0 || self.risk.max_short_pct > 1.0 {
            return Err(Error::Config(
                "max_short_pct must be in [0.0, 1.0]".into(),
            ));
        }
        Ok(())
    }

    /// IBKR connection address string.
    pub fn address(&self) -> String {
        format!("{}:{}", self.connection.host, self.connection.port)
    }

    /// Full path to the audit log file.
    pub fn audit_path(&self) -> std::path::PathBuf {
        Path::new(&self.logging.dir).join(&self.logging.audit_file)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn example_toml() -> &'static str {
        r#"
[connection]
host = "127.0.0.1"
port = 4002
client_id = 100

[account]
id = "DU123456"
type = "margin"

[execution]
order_interval_ms = 100
limit_offset_bps = 5
order_timeout_secs = 300
max_orders_per_run = 50

[risk]
max_position_pct = 0.25
max_leverage = 1.5
min_trade_usd = 100.0
max_trade_usd = 100000.0
allow_short = true
max_short_pct = 0.30

[cost]
commission_per_share = 0.0035
commission_min = 0.35
slippage_bps = 5

[logging]
dir = "./logs"
audit_file = "audit.jsonl"
"#
    }

    #[test]
    fn parse_example_config() {
        let config: Config = toml::from_str(example_toml()).unwrap();
        assert_eq!(config.connection.port, 4002);
        assert_eq!(config.connection.client_id, 100);
        assert_eq!(config.account.account_type, AccountType::Margin);
        assert_eq!(config.execution.order_interval_ms, 100);
        assert_eq!(config.risk.max_position_pct, 0.25);
        assert_eq!(config.cost.commission_per_share, 0.0035);
    }

    #[test]
    fn validate_catches_bad_port() {
        let mut config: Config = toml::from_str(example_toml()).unwrap();
        config.connection.port = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_catches_bad_max_position() {
        let mut config: Config = toml::from_str(example_toml()).unwrap();
        config.risk.max_position_pct = 1.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn validate_catches_bad_leverage() {
        let mut config: Config = toml::from_str(example_toml()).unwrap();
        config.risk.max_leverage = 0.5;
        assert!(config.validate().is_err());
    }

    #[test]
    fn address_format() {
        let config: Config = toml::from_str(example_toml()).unwrap();
        assert_eq!(config.address(), "127.0.0.1:4002");
    }

    #[test]
    fn audit_path() {
        let config: Config = toml::from_str(example_toml()).unwrap();
        assert_eq!(
            config.audit_path(),
            std::path::PathBuf::from("./logs/audit.jsonl")
        );
    }

    #[test]
    fn cash_account_type() {
        let toml = example_toml().replace("\"margin\"", "\"cash\"");
        let config: Config = toml::from_str(&toml).unwrap();
        assert_eq!(config.account.account_type, AccountType::Cash);
    }
}
