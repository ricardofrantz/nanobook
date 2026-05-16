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
    use std::io;
    use std::sync::{Arc, Mutex};
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct CapturedLogs(Arc<Mutex<Vec<u8>>>);

    struct CapturedWriter(Arc<Mutex<Vec<u8>>>);

    impl io::Write for CapturedWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for CapturedLogs {
        type Writer = CapturedWriter;

        fn make_writer(&'a self) -> Self::Writer {
            CapturedWriter(Arc::clone(&self.0))
        }
    }

    impl CapturedLogs {
        fn lines(&self) -> Vec<serde_json::Value> {
            let bytes = self.0.lock().unwrap().clone();
            let text = String::from_utf8(bytes).unwrap();
            text.lines()
                .filter(|line| !line.trim().is_empty())
                .map(|line| serde_json::from_str(line).unwrap())
                .collect()
        }
    }

    #[test]
    fn correlation_id_generation_is_unique_and_prefixed() {
        let first = generate_correlation_id();
        let second = generate_correlation_id();
        assert!(first.starts_with("rebalance-"));
        assert_ne!(first, second);
    }

    #[test]
    fn json_log_output_is_parseable() {
        let captured = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_writer(captured.clone())
            .finish();

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!(answer = 42, "structured test event");
        });

        let lines = captured.lines();
        assert_eq!(lines.len(), 1);
        assert_eq!(lines[0]["level"], "INFO");
        assert_eq!(lines[0]["fields"]["message"], "structured test event");
        assert_eq!(lines[0]["fields"]["answer"], 42);
    }

    #[test]
    fn json_log_output_includes_span_context_and_correlation_id() {
        let captured = CapturedLogs::default();
        let subscriber = tracing_subscriber::fmt()
            .json()
            .with_current_span(true)
            .with_span_list(true)
            .with_writer(captured.clone())
            .finish();
        let correlation_id = "rebalance-test-correlation";

        tracing::subscriber::with_default(subscriber, || {
            let span = tracing::info_span!(
                "rebalance_run",
                correlation_id,
                target_file = "target.json",
                account = "DU123",
            );
            let _guard = span.enter();
            tracing::info!(phase = "risk_check", "inside run span");
        });

        let lines = captured.lines();
        assert_eq!(lines.len(), 1);
        let spans = lines[0]["spans"].as_array().unwrap();
        assert_eq!(spans[0]["name"], "rebalance_run");
        assert_eq!(spans[0]["correlation_id"], correlation_id);
        assert_eq!(lines[0]["fields"]["phase"], "risk_check");
    }
}
