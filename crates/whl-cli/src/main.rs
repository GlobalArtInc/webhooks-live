use std::process::ExitCode;

use clap::Parser;
use whl_cli::cli::Cli;
use whl_cli::output::{MAX_MESSAGE_CHARS, safe_to};

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = Cli::parse();
    match whl_cli::run(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", safe_to(&error.to_string(), MAX_MESSAGE_CHARS));
            ExitCode::from(error.status().code())
        }
    }
}
