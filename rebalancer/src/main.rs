//! CLI entry point for the nanobook rebalancer.

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use nanobook_broker::Broker;
use nanobook_broker::ibkr::IbkrBroker;
use nanobook_rebalancer::config::Config;
use nanobook_rebalancer::error::Error;
use nanobook_rebalancer::execution::{self, RunOptions};
use nanobook_rebalancer::recovery;
use nanobook_rebalancer::target::TargetSpec;
use nanobook_rebalancer::validator;

#[derive(Parser)]
#[command(name = "rebalancer")]
#[command(about = "Portfolio rebalancer: nanobook → Interactive Brokers")]
#[command(version)]
struct Cli {
    /// Path to config.toml
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

    /// Skip startup validation checks. Intended for tests and emergency diagnostics, not production.
    #[arg(long)]
    skip_validation: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Compute diff, confirm, and execute rebalance orders
    Run {
        /// Path to target.json
        target: PathBuf,

        /// Show plan without executing
        #[arg(long)]
        dry_run: bool,

        /// Skip confirmation prompt (for automation/cron)
        #[arg(long)]
        force: bool,

        /// Enable cron mode with idempotency checks (for automation)
        ///
        /// In cron mode, the rebalancer writes a sequence number to the audit log
        /// for each rebalance window. If the same window is already complete,
        /// subsequent runs are rejected to prevent double-firing. This is designed
        /// for automated cron jobs that may run multiple times for the same target.
        #[arg(long)]
        cron_mode: bool,
    },

    /// Show current IBKR positions
    Positions,

    /// Check IBKR connection
    Status,

    /// Compare actual positions vs target
    Reconcile {
        /// Path to target.json
        target: PathBuf,
    },

    /// Send SIGTERM to running runner and verify no dangling orders
    Kill,

    /// Recover from a crash using audit log
    Recover {
        /// Path to target.json (required for resume)
        target: PathBuf,

        /// Show recovery plan without executing
        #[arg(long)]
        dry_run: bool,
    },
}

fn main() {
    if let Err(e) = nanobook_rebalancer::observability::init_tracing("logs") {
        eprintln!("Error initializing tracing: {e}");
        process::exit(1);
    }

    let cli = Cli::parse();

    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            process::exit(1);
        }
    };

    if validator::should_run_startup_validation(cli.skip_validation) {
        if let Err(e) = validator::validate_static_or_error(&config) {
            eprintln!("{e}");
            process::exit(1);
        }
    }

    let result = match cli.command {
        Command::Run {
            target,
            dry_run,
            force,
            cron_mode,
        } => {
            let spec = match TargetSpec::load(&target) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error loading target: {e}");
                    process::exit(1);
                }
            };
            let opts = RunOptions {
                dry_run,
                force,
                target_file: target.display().to_string(),
                cron_mode,
            };
            execution::run(&config, &spec, &opts)
        }
        Command::Positions => execution::show_positions(&config),
        Command::Status => execution::check_status(&config),
        Command::Reconcile { target } => {
            let spec = match TargetSpec::load(&target) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error loading target: {e}");
                    process::exit(1);
                }
            };
            execution::run_reconcile(&config, &spec)
        }
        Command::Kill => nanobook_rebalancer::kill::run_kill(&config),
        Command::Recover { target, dry_run } => {
            let spec = match TargetSpec::load(&target) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error loading target: {e}");
                    process::exit(1);
                }
            };

            // Try to connect to broker for state comparison
            let mut broker = IbkrBroker::new(
                &config.connection.host,
                config.connection.port,
                config.connection.client_id,
            );
            let broker_ref = match broker.connect() {
                Ok(()) => {
                    println!("Connected to IBKR for broker state comparison.");
                    Some(&broker as &dyn Broker)
                }
                Err(e) => {
                    eprintln!("Warning: Failed to connect to IBKR: {e}");
                    eprintln!("Proceeding with recovery without broker state comparison.");
                    None
                }
            };

            recovery::run_recover(&config, &spec, dry_run, broker_ref)
        }
    };

    if let Err(e) = result {
        match &e {
            Error::RiskFailed(msg) => {
                eprintln!("\nAborted: {msg}");
                process::exit(2);
            }
            Error::Aborted(msg) => {
                eprintln!("{msg}");
                process::exit(0);
            }
            _ => {
                eprintln!("Error: {e}");
                process::exit(1);
            }
        }
    }
}
