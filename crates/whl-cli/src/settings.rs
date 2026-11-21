use std::path::PathBuf;

use reqwest::header::{HeaderName, HeaderValue};
use url::{Host, Url};

use crate::api::ApiSettings;
use crate::auth;
use crate::cli::{ForwardArgs, FromArg};
use crate::error::CliError;
use crate::filter::Filters;
use crate::model::{EndpointId, Token};
use crate::paths::Environment;
use crate::replay::{HeaderRules, ReplayConfig};

pub const DEFAULT_SERVER: &str = "https://webhooks.live";
const UI_SEGMENT: &str = "e";

pub struct Settings {
    pub api: ApiSettings,
    pub replay: ReplayConfig,
    pub filters: Filters,
    pub from: Option<FromArg>,
    pub state_path: PathBuf,
    pub server_label: String,
    pub dry_run: bool,
    pub verbose: bool,
}

fn usage(message: impl Into<String>) -> CliError {
    CliError::Usage(message.into())
}

fn origin_of(url: &Url) -> Result<Url, CliError> {
    if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
        return Err(usage("the server URL must be an http or https URL"));
    }
    Url::parse(&url.origin().ascii_serialization())
        .map_err(|error| usage(format!("the server URL is not usable: {error}")))
}

fn server_from_option(server: &Url) -> Result<Url, CliError> {
    let bare_path = matches!(server.path(), "" | "/");
    if !bare_path || server.query().is_some() || server.fragment().is_some() {
        return Err(usage(
            "the server URL must not have a path, query or fragment",
        ));
    }
    origin_of(server)
}

pub fn server_base(server: Option<&Url>) -> Result<Url, CliError> {
    match server {
        Some(server) => server_from_option(server),
        None => origin_of(&Url::parse(DEFAULT_SERVER).map_err(|error| usage(error.to_string()))?),
    }
}

pub fn parse_endpoint(argument: &str, server: Option<&Url>) -> Result<(Url, EndpointId), CliError> {
    if !argument.contains("://") {
        let base = server_base(server)?;
        let id = argument
            .parse()
            .map_err(|error| usage(format!("{error}")))?;
        return Ok((base, id));
    }
    let url = Url::parse(argument)
        .map_err(|error| usage(format!("the endpoint URL is not valid: {error}")))?;
    let base = origin_of(&url)?;
    if let Some(server) = server
        && server_from_option(server)? != base
    {
        return Err(usage(
            "the capture URL and --server name different servers; pass only one of them",
        ));
    }
    let mut segments = url.path_segments().into_iter().flatten();
    let first = segments.next().unwrap_or_default();
    let candidate = if first == UI_SEGMENT {
        segments.next().unwrap_or_default()
    } else {
        first
    };
    let id = candidate.parse().map_err(|error| {
        usage(format!(
            "the first path segment of the endpoint URL is not an endpoint id: {error}"
        ))
    })?;
    Ok((base, id))
}

fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(name)) => {
            let name = name.to_ascii_lowercase();
            name == "localhost"
        }
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

pub fn check_token_transport(base: &Url, token: Option<&Token>) -> Result<(), CliError> {
    if token.is_some() && base.scheme() != "https" && !is_loopback(base) {
        return Err(usage(
            "refusing to send an API token over plain http; use an https server URL",
        ));
    }
    Ok(())
}

pub fn check_target(target: &Url) -> Result<(), CliError> {
    if !matches!(target.scheme(), "http" | "https") || target.host_str().is_none() {
        return Err(usage("--to must be an http or https URL"));
    }
    if !target.username().is_empty() || target.password().is_some() {
        return Err(usage(
            "--to must not carry credentials; use --header 'Authorization: ...' instead",
        ));
    }
    if target.query().is_some() || target.fragment().is_some() {
        return Err(usage("--to must not have a query or a fragment"));
    }
    Ok(())
}

fn parse_header(text: &str) -> Result<(HeaderName, HeaderValue), CliError> {
    let shape = "--header must look like 'Name: value'";
    let (name, value) = text.split_once(':').ok_or_else(|| usage(shape))?;
    let name = HeaderName::from_bytes(name.trim().as_bytes())
        .map_err(|_| usage("--header has an invalid header name"))?;
    let value = HeaderValue::from_str(value.trim_start())
        .map_err(|_| usage("--header has an invalid header value"))?;
    Ok((name, value))
}

fn prefix(option: &str, value: Option<String>) -> Result<Option<String>, CliError> {
    match value {
        Some(value) if !value.starts_with('/') => Err(usage(format!("{option} must start with /"))),
        other => Ok(other),
    }
}

pub fn server_label(base: &Url) -> String {
    base.as_str().trim_end_matches('/').to_owned()
}

pub fn check_explicit_token_server(base: &Url, server: Option<&Url>) -> Result<(), CliError> {
    let default = server_base(None)?;
    let chosen = match server {
        Some(server) => Some(server_from_option(server)?),
        None => None,
    };
    if *base == default || chosen.as_ref() == Some(base) {
        return Ok(());
    }
    Err(CliError::TokenRefused(format!(
        "refusing to send the token from --token or WEBHOOKS_LIVE_TOKEN to {}, which is neither the default server nor --server; pass --server {} to confirm",
        server_label(base),
        server_label(base)
    )))
}

fn token_for(
    args_token: Option<String>,
    environment: &Environment,
    base: &Url,
    server: Option<&Url>,
) -> Result<Option<Token>, CliError> {
    if let Some(text) = args_token {
        let token = text
            .parse()
            .map_err(|error| usage(format!("--token is refused: {error}")))?;
        check_explicit_token_server(base, server)?;
        return Ok(Some(token));
    }
    let Ok(path) = environment.token_file() else {
        return Ok(None);
    };
    match auth::load_token(&path)? {
        Some(stored) if stored.server == server_label(base) => Ok(Some(stored.token)),
        Some(stored) => {
            eprintln!(
                "note: the stored token belongs to {}, so it is not sent to {}",
                stored.server,
                server_label(base)
            );
            Ok(None)
        }
        None => Ok(None),
    }
}

pub fn resolve(args: ForwardArgs, environment: &Environment) -> Result<Settings, CliError> {
    let (base, endpoint) = parse_endpoint(&args.endpoint, args.server.as_ref())?;
    let token = token_for(args.token, environment, &base, args.server.as_ref())?;
    check_token_transport(&base, token.as_ref())?;
    check_target(&args.to)?;

    let set = args
        .header
        .iter()
        .map(|text| parse_header(text))
        .collect::<Result<Vec<_>, _>>()?;
    let remove = args
        .remove_header
        .iter()
        .map(|name| {
            HeaderName::from_bytes(name.trim().as_bytes())
                .map_err(|_| usage("--remove-header has an invalid header name"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let filters = Filters::new(args.method, prefix("--path", args.path)?);
    let strip_prefix = prefix("--strip-prefix", args.strip_prefix)?;

    let server_label = server_label(&base);
    let state_path = match args.state_file {
        Some(path) => path,
        None => environment.state_file(&base, endpoint.unpack())?,
    };

    Ok(Settings {
        api: ApiSettings {
            base,
            endpoint,
            token,
        },
        replay: ReplayConfig {
            target: args.to,
            strip_prefix,
            headers: HeaderRules {
                keep_host: args.keep_host,
                set,
                remove,
            },
        },
        filters,
        from: args.from,
        state_path,
        server_label,
        dry_run: args.dry_run,
        verbose: args.verbose,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

    fn url(text: &str) -> Url {
        Url::parse(text).expect("url")
    }

    #[test]
    fn resolves_a_bare_id_against_the_default_server() {
        let (base, id) = parse_endpoint(ID, None).expect("endpoint");
        assert_eq!(base.as_str(), "https://webhooks.live/");
        assert_eq!(id.unpack(), ID);
    }

    #[test]
    fn takes_the_server_and_the_id_from_a_capture_url() {
        let (base, id) = parse_endpoint(&format!("http://127.0.0.1:8080/{ID}/orders?x=1"), None)
            .expect("endpoint");
        assert_eq!(base.as_str(), "http://127.0.0.1:8080/");
        assert_eq!(id.unpack(), ID);
    }

    #[test]
    fn accepts_the_interface_url_of_an_endpoint() {
        let (_, id) =
            parse_endpoint(&format!("https://webhooks.live/e/{ID}"), None).expect("endpoint");
        assert_eq!(id.unpack(), ID);
    }

    #[test]
    fn refuses_a_capture_url_and_a_different_server() {
        let result = parse_endpoint(
            &format!("https://webhooks.live/{ID}"),
            Some(&url("https://other.example")),
        );
        assert!(matches!(result, Err(CliError::Usage(_))));
    }

    #[test]
    fn refuses_a_server_with_a_path() {
        let result = parse_endpoint(ID, Some(&url("https://example.com/prefix")));
        assert!(matches!(result, Err(CliError::Usage(_))));
    }

    #[test]
    fn refuses_to_send_a_token_over_plain_http_except_to_loopback() {
        let token: Token = "whl_x".parse().expect("token");
        assert!(check_token_transport(&url("http://example.com"), Some(&token)).is_err());
        assert!(check_token_transport(&url("http://127.0.0.1:8080"), Some(&token)).is_ok());
        assert!(check_token_transport(&url("http://localhost"), Some(&token)).is_ok());
        assert!(check_token_transport(&url("https://example.com"), Some(&token)).is_ok());
        assert!(check_token_transport(&url("http://example.com"), None).is_ok());
    }

    #[test]
    fn sends_an_explicit_token_only_to_the_default_or_the_chosen_server() {
        let default = url("https://webhooks.live/");
        let other = url("https://other.example/");
        assert!(check_explicit_token_server(&default, None).is_ok());
        assert!(check_explicit_token_server(&other, Some(&url("https://other.example"))).is_ok());
        let refused = check_explicit_token_server(&other, None);
        assert!(matches!(refused, Err(CliError::TokenRefused(_))));
        assert_eq!(refused.expect_err("refused").status().code(), 3);
    }

    #[test]
    fn treats_only_localhost_and_loopback_addresses_as_loopback() {
        assert!(is_loopback(&url("http://localhost:1")));
        assert!(is_loopback(&url("http://[::1]:1")));
        assert!(!is_loopback(&url("http://evil.localhost:1")));
    }

    #[test]
    fn refuses_a_target_with_credentials_or_a_query() {
        assert!(check_target(&url("http://user:pw@localhost:3000")).is_err());
        assert!(check_target(&url("http://localhost:3000/?a=1")).is_err());
        assert!(check_target(&url("ftp://localhost")).is_err());
        assert!(check_target(&url("http://localhost:3000/hooks")).is_ok());
    }

    #[test]
    fn parses_a_header_option_without_echoing_its_value_on_failure() {
        let (name, value) = parse_header("X-Env:  local").expect("header");
        assert_eq!(name.as_str(), "x-env");
        assert_eq!(value, "local");
        let error = parse_header("no colon with secret").expect_err("refused");
        assert!(!error.to_string().contains("secret"));
    }
}
