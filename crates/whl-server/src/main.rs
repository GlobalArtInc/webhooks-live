use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whl_core::config::Config;

#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;

#[derive(Debug, Parser)]
#[command(
    name = "webhooks-live",
    version,
    about = "Self-hosted HTTP request catcher"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Run the public and admin listeners")]
    Serve,
}

#[tokio::main]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Serve => serve().await,
    }
}

async fn serve() -> ExitCode {
    let config = match Config::load() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("webhooks-live: {error}");
            return ExitCode::from(2);
        }
    };
    if let Err(error) = whl_telemetry::logs::init(&config.log) {
        eprintln!("webhooks-live: {error}");
        return ExitCode::from(2);
    }
    if let Err(error) = whl_telemetry::metrics::install() {
        tracing::error!(%error, "metrics are unavailable");
        return ExitCode::from(2);
    }

    match whl_server::run(config, whl_server::shutdown_signal()).await {
        Ok(()) => {
            tracing::info!("stopped");
            ExitCode::SUCCESS
        }
        Err(error) => {
            tracing::error!(%error, "server failed");
            ExitCode::FAILURE
        }
    }
}
