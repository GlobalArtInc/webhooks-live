pub mod api;
pub mod auth;
pub mod backoff;
pub mod cli;
pub mod error;
pub mod filter;
pub mod forward;
pub mod fsio;
pub mod model;
pub mod output;
pub mod paths;
pub mod replay;
pub mod settings;
pub mod shutdown;
pub mod sse;
pub mod state;

use std::io::{self, IsTerminal, Write};

use api::Api;
use cli::{Cli, Command, ForwardArgs, FromArg, LoginArgs};
use error::CliError;
use forward::Forwarder;
use output::Output;
use paths::Environment;
use replay::Replayer;
use settings::Settings;
use state::State;

pub async fn run(cli: Cli) -> Result<(), CliError> {
    let environment = Environment::capture();
    match cli.command {
        Command::Forward(args) => forward(*args, &environment).await,
        Command::Login(args) => login(args, &environment),
        Command::Logout => logout(&environment),
    }
}

async fn forward(args: ForwardArgs, environment: &Environment) -> Result<(), CliError> {
    let Settings {
        api,
        replay,
        filters,
        from,
        state_path,
        server_label,
        dry_run,
        verbose,
    } = settings::resolve(args, environment)?;
    let output = Output::new(verbose);
    let endpoint = api.endpoint.unpack().to_owned();
    let state = State::open(state_path, &server_label, &endpoint, !dry_run)?;
    let cursor = match from {
        Some(FromArg::Now) => None,
        Some(FromArg::All) => Some(0),
        Some(FromArg::After(seq)) => Some(seq),
        None => state.last_seq(),
    };
    output.notice(&format!(
        "forwarding {server_label}/{endpoint} to {}",
        replay.target
    ))?;
    if dry_run {
        output.notice("dry run: nothing is sent and the state file is not written")?;
    } else {
        output.notice(&format!("state file: {}", state.path().display()))?;
    }
    let replayer = Replayer::new(replay).map_err(|error| CliError::Runtime(error.to_string()))?;
    let api = Api::new(api)?;
    let shutdown = shutdown::install(output)?;
    Forwarder {
        api,
        endpoint,
        replayer,
        filters,
        state,
        output,
        dry_run,
        shutdown,
        cursor,
        failing: None,
    }
    .run()
    .await
}

fn login(args: LoginArgs, environment: &Environment) -> Result<(), CliError> {
    let base = settings::server_base(args.server.as_ref())?;
    let stdin = io::stdin();
    if stdin.is_terminal() {
        return Err(CliError::Usage(
            "stdin is a terminal and would echo the token; pipe it in, for example: pbpaste | webhooks-live-cli login"
                .to_owned(),
        ));
    }
    let token = auth::read_token(stdin.lock())?;
    let path = environment.token_file()?;
    let label = settings::server_label(&base);
    auth::store_token(&path, &label, &token)?;
    writeln!(
        io::stdout().lock(),
        "token stored in {} for {label}",
        path.display()
    )?;
    Ok(())
}

fn logout(environment: &Environment) -> Result<(), CliError> {
    let path = environment.token_file()?;
    let message = if auth::remove_token(&path)? {
        format!("removed {}", path.display())
    } else {
        "no stored token".to_owned()
    };
    writeln!(io::stdout().lock(), "{message}")?;
    Ok(())
}
