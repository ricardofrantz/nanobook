//! Structured tracing setup for the rebalancer CLI.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt;
use tracing_subscriber::prelude::*;

use crate::error::Result;

static LOG_GUARD: OnceLock<tracing_appender::non_blocking::WorkerGuard> = OnceLock::new();

/// Generate a process-local correlation ID suitable for tagging a run span.
pub fn generate_correlation_id() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or_default();
    format!("rebalance-{nanos}-{}", std::process::id())
}

/// Initialize tracing with JSON stdout and a rolling general log file.
///
/// The audit JSONL file remains separate and is still written by `audit.rs`.
/// Existing `log` crate calls are bridged through `tracing-log` so migration can
/// happen incrementally in later observability beads.
pub fn init_tracing(log_dir: impl AsRef<Path>) -> Result<()> {
    let _ = tracing_log::LogTracer::init();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let stdout = fmt::layer().json().with_writer(std::io::stdout);

    let log_dir: PathBuf = log_dir.as_ref().to_path_buf();
    std::fs::create_dir_all(&log_dir)?;
    let appender = tracing_appender::rolling::daily(log_dir, "rebalancer.log");
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let _ = LOG_GUARD.set(guard);
    let file = fmt::layer().json().with_writer(file_writer);

    let subscriber = tracing_subscriber::registry()
        .with(filter)
        .with(stdout)
        .with(file);

    tracing::subscriber::set_global_default(subscriber)
        .map_err(|e| crate::error::Error::Config(format!("failed to initialize tracing: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn correlation_id_generation_is_unique_and_prefixed() {
        let first = generate_correlation_id();
        let second = generate_correlation_id();
        assert!(first.starts_with("rebalance-"));
        assert_ne!(first, second);
    }
}
