//! CLI entry point for the nanobook rebalancer.

use std::path::PathBuf;
use std::process;

use clap::{Parser, Subcommand};

use nanobook_rebalancer::config::Config;
use nanobook_rebalancer::error::Error;
use nanobook_rebalancer::execution::{self, RunOptions};
use nanobook_rebalancer::target::TargetSpec;

#[derive(Parser)]
#[command(name = "rebalancer")]
#[command(about = "Portfolio rebalancer: nanobook → Interactive Brokers")]
#[command(version)]
struct Cli {
    /// Path to config.toml
    #[arg(long, default_value = "config.toml")]
    config: PathBuf,

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
}

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_secs()
        .init();

    let cli = Cli::parse();

    let config = match Config::load(&cli.config) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config: {e}");
            process::exit(1);
        }
    };

    let result = match cli.command {
        Command::Run {
            target,
            dry_run,
            force,
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
