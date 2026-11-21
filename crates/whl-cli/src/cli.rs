use std::path::PathBuf;
use std::str::FromStr;

use clap::{Args, Parser, Subcommand};
use url::Url;

const ABOUT: &str = "Replay the captures of a webhooks.live endpoint to a local URL as they arrive";

const LONG_ABOUT: &str =
    "Replay the captures of a webhooks.live endpoint to a local URL as they arrive.

The program opens the endpoint's live stream, fetches every capture with its raw body and sends it \
to --to with the same method, sub-path, query, headers (minus hop-by-hop ones) and body bytes. It \
reconnects with backoff and resumes from the last forwarded sequence number, which it keeps in a \
state file, so a restart does not lose captures. Delivery is at least once: a crash between the \
local request and the state write repeats one capture.";

const FORWARD_AFTER_HELP: &str = "Local targets are not restricted:
  --to is deliberately NOT checked by the SSRF guard that the server-side forwarder applies. This \
program runs on your own machine and replays to whatever --to names, including localhost and \
private networks. Anyone who knows the endpoint id can send a request that is replayed to --to, \
so point it only at a service you would accept those requests on. A captured path that would leave \
the --to path prefix (for example through '..') is refused.

Starting point (--from):
  default      resume after the last forwarded sequence number in the state file; without a state \
file, start with the next capture
  now          ignore the state file and start with the next capture
  all          replay the whole history that the server still holds, oldest first
  SEQ          replay captures with a sequence number greater than SEQ

Exit codes:
  0  stopped by Ctrl-C or SIGTERM
  1  unexpected failure
  2  invalid arguments
  3  the server refused the token (401 or 403)
  4  the endpoint is gone, expired or deleted
  5  the state file is unreadable, unwritable, foreign or used by another forwarder

Failures of the local target are printed and the capture counts as handled; they do not stop the \
program.";

#[derive(Parser)]
#[command(name = "webhooks-live-cli", version, about = ABOUT, long_about = LONG_ABOUT)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[command(after_long_help = FORWARD_AFTER_HELP)]
    Forward(Box<ForwardArgs>),
    #[command(
        about = "Store an API token (read from stdin) for one server in the user config directory",
        long_about = "Reads one line from stdin and stores it, mode 0600, together with the server it belongs to. The stored token is sent only to that server. Pipe the token in so that it is not echoed or kept in shell history, for example: pbpaste | webhooks-live-cli login"
    )]
    Login(LoginArgs),
    #[command(about = "Delete the stored API token")]
    Logout,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromArg {
    Now,
    All,
    After(u64),
}

impl FromStr for FromArg {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        match text {
            "now" => Ok(Self::Now),
            "all" => Ok(Self::All),
            other => other
                .parse()
                .map(Self::After)
                .map_err(|_| "expected now, all or a sequence number".to_owned()),
        }
    }
}

#[derive(Args)]
pub struct LoginArgs {
    #[arg(
        long,
        value_name = "URL",
        env = "WEBHOOKS_LIVE_SERVER",
        help = "Server the token belongs to [default: https://webhooks.live]"
    )]
    pub server: Option<Url>,
}

#[derive(Args)]
pub struct ForwardArgs {
    #[arg(
        value_name = "ENDPOINT",
        help = "Endpoint id, or the capture URL (https://host/<id>) of the endpoint"
    )]
    pub endpoint: String,

    #[arg(
        long,
        value_name = "URL",
        help = "Local URL that receives the replayed requests",
        long_help = "Local URL that receives the replayed requests. The check that a captured path stays under the path of this URL is syntactic: encodings that only the target decodes (for example %2f or double-encoded dots) are passed on as captured."
    )]
    pub to: Url,

    #[arg(
        long,
        value_name = "URL",
        env = "WEBHOOKS_LIVE_SERVER",
        help = "Server of the endpoint when ENDPOINT is a bare id [default: https://webhooks.live]"
    )]
    pub server: Option<Url>,

    #[arg(
        long,
        value_name = "TOKEN",
        env = "WEBHOOKS_LIVE_TOKEN",
        hide_env_values = true,
        help = "API token, sent as 'Authorization: Bearer'; prefer the env var or `login` (argv is visible to other users); anonymous endpoints need none"
    )]
    pub token: Option<String>,

    #[arg(long, value_name = "now|all|SEQ", help = "Where to start (see below)")]
    pub from: Option<FromArg>,

    #[arg(
        long,
        value_name = "METHODS",
        value_delimiter = ',',
        help = "Forward only these methods, comma separated"
    )]
    pub method: Vec<String>,

    #[arg(
        long,
        value_name = "PREFIX",
        help = "Forward only captures whose sub-path starts with PREFIX (for example /stripe)"
    )]
    pub path: Option<String>,

    #[arg(
        long,
        value_name = "PREFIX",
        help = "Remove PREFIX from the sub-path before appending it to --to"
    )]
    pub strip_prefix: Option<String>,

    #[arg(long, help = "Send the captured Host header instead of the target's")]
    pub keep_host: bool,

    #[arg(
        short = 'H',
        long = "header",
        value_name = "'NAME: VALUE'",
        help = "Set a header on every replayed request, replacing the captured one; repeatable"
    )]
    pub header: Vec<String>,

    #[arg(
        long = "remove-header",
        value_name = "NAME",
        help = "Do not send this captured header; repeatable"
    )]
    pub remove_header: Vec<String>,

    #[arg(
        long,
        help = "Print what would be sent; send nothing and do not touch the state file"
    )]
    pub dry_run: bool,

    #[arg(
        long,
        value_name = "PATH",
        help = "State file [default: <state dir>/webhooks-live/<server>/<endpoint id>.json]"
    )]
    pub state_file: Option<PathBuf>,

    #[arg(short, long, help = "Also print captures that the filters skipped")]
    pub verbose: bool,
}
