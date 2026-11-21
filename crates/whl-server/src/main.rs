use std::process::ExitCode;

use clap::{Parser, Subcommand};
use whl_core::config::Config;
use whl_store::{Store, StoreError};

#[global_allocator]
static GLOBAL: whl_script::CountingAlloc<mimalloc::MiMalloc> =
    whl_script::CountingAlloc::new(mimalloc::MiMalloc);

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
    #[command(about = "Maintain the encrypted endpoint secrets")]
    Secrets {
        #[command(subcommand)]
        command: SecretsCommand,
    },
}

#[derive(Debug, Subcommand)]
enum SecretsCommand {
    #[command(about = "Re-seal every stored secret under the active key")]
    Rotate {
        #[arg(
            long,
            default_value_t = whl_server::DEFAULT_BATCH_SIZE,
            value_parser = clap::value_parser!(u32).range(1..=i64::from(whl_server::MAX_BATCH_SIZE)),
            help = "Rows re-sealed per batch"
        )]
        batch_size: u32,
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
        Command::Secrets { command } => secrets(command).await,
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

async fn secrets(command: SecretsCommand) -> ExitCode {
    let SecretsCommand::Rotate { batch_size } = command;
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
    let keyring = match whl_server::rotation_keyring(&config) {
        Ok(keyring) => keyring,
        Err(error) => {
            eprintln!("webhooks-live: {error}");
            return ExitCode::from(2);
        }
    };
    let store = match Store::connect(&config.database).await {
        Ok(store) => store,
        Err(error) => {
            tracing::error!(%error, "secrets rotate failed");
            return ExitCode::FAILURE;
        }
    };
    if let Err(error) = store.check_schema_version().await {
        tracing::error!(%error, "secrets rotate failed");
        return ExitCode::FAILURE;
    }
    println!(
        "re-sealing every secret under key {} in batches of {batch_size}",
        keyring.active_id()
    );
    let outcome = whl_server::rotate_secrets(&store, &keyring, batch_size, |progress| {
        println!(
            "batch {}: rotated {} of {} found at the start, skipped {}, failed {}",
            progress.report.batches,
            progress.report.rotated,
            progress.pending_at_start,
            progress.report.skipped,
            progress.report.failed,
        );
    })
    .await;
    let report = match outcome {
        Ok(report) => report,
        Err(error) => {
            tracing::error!(%error, "secrets rotate failed");
            return ExitCode::FAILURE;
        }
    };
    let remaining = match store.count_secrets_outside_key(keyring.active_id()).await {
        Ok(remaining) => remaining,
        Err(error) => {
            tracing::error!(%error, "secrets rotate failed");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "done: rotated {}, skipped {} changed meanwhile, failed {}, {remaining} rows remain under another key",
        report.rotated, report.skipped, report.failed
    );
    if report.failed > 0 {
        eprintln!(
            "webhooks-live: {} secrets could not be rotated and were left untouched, see the warnings above",
            report.failed
        );
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
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
