use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whl_core::config::Config;
use whl_store::{Store, StoreError};

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
    #[command(about = "Apply, take back, or list database migrations")]
    Migrate {
        #[command(subcommand)]
        command: MigrateCommand,
    },
}

#[derive(Debug, Subcommand)]
enum MigrateCommand {
    #[command(about = "Apply every pending migration")]
    Up,
    #[command(about = "Take back the last applied migration")]
    Down {
        #[arg(long, help = "Confirm that the last applied migration is taken back")]
        yes: bool,
    },
    #[command(about = "List applied and pending migrations")]
    Status,
}

#[tokio::main]
async fn main() -> ExitCode {
    match Cli::parse().command {
        Command::Serve => serve().await,
        Command::Migrate { command } => migrate(command).await,
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

async fn migrate(command: MigrateCommand) -> ExitCode {
    if matches!(command, MigrateCommand::Down { yes: false }) {
        eprintln!(
            "webhooks-live: migrate down takes back the last applied migration; pass --yes to confirm"
        );
        return ExitCode::from(2);
    }
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

    let store = match Store::connect(&config.database).await {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, "migrate failed");
            return ExitCode::FAILURE;
        }
    };
    let outcome = match command {
        MigrateCommand::Up => migrate_up(&store).await,
        MigrateCommand::Down { .. } => migrate_down(&store).await,
        MigrateCommand::Status => migrate_status(&store).await,
    };
    match outcome {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "migrate failed");
            ExitCode::FAILURE
        }
    }
}

async fn migrate_up(store: &Store) -> Result<(), StoreError> {
    let applied = store.migrate_up().await?;
    if applied.is_empty() {
        println!("the schema is up to date");
    }
    for name in applied {
        println!("applied {name}");
    }
    Ok(())
}

async fn migrate_down(store: &Store) -> Result<(), StoreError> {
    match store.migrate_down().await? {
        Some(name) => println!("reverted {name}"),
        None => println!("no migration is applied"),
    }
    Ok(())
}

async fn migrate_status(store: &Store) -> Result<(), StoreError> {
    let status = store.migration_status().await?;
    for migration in status.migrations {
        let state = if migration.applied {
            "applied"
        } else {
            "pending"
        };
        println!("{state:<8} {}", migration.name);
    }
    for name in status.unknown {
        println!("unknown  {name} (recorded in the database, not part of this build)");
    }
    Ok(())
}
