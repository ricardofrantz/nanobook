//! Error types for the rebalancer.

use std::path::PathBuf;

/// All errors that can occur during rebalancer operation.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("config error: {0}")]
    Config(String),

    #[error("failed to read config file {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse config: {0}")]
    ConfigParse(#[from] toml::de::Error),

    #[error("target file error: {0}")]
    Target(String),

    #[error("failed to read target file {path}: {source}")]
    TargetRead {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("failed to parse target JSON: {0}")]
    TargetParse(#[from] serde_json::Error),

    #[error("risk check failed: {0}")]
    RiskFailed(String),

    #[error("IBKR connection error: {0}")]
    Connection(String),

    #[error("IBKR order error: {0}")]
    Order(String),

    #[error("execution aborted: {0}")]
    Aborted(String),

    #[error("reconciliation error: {0}")]
    Reconcile(String),

    #[error("audit log error: {0}")]
    Audit(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;
