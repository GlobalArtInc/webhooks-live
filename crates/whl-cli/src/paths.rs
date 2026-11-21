use std::ffi::OsString;
use std::path::PathBuf;

use url::Url;

use crate::error::CliError;

const APP_DIR: &str = "webhooks-live";
const TOKEN_FILE: &str = "token";

#[derive(Debug, Clone, Default)]
pub struct Environment {
    pub xdg_state_home: Option<OsString>,
    pub xdg_config_home: Option<OsString>,
    pub home: Option<OsString>,
    pub local_app_data: Option<OsString>,
    pub app_data: Option<OsString>,
}

impl Environment {
    pub fn capture() -> Self {
        Self {
            xdg_state_home: std::env::var_os("XDG_STATE_HOME"),
            xdg_config_home: std::env::var_os("XDG_CONFIG_HOME"),
            home: std::env::var_os("HOME"),
            local_app_data: std::env::var_os("LOCALAPPDATA"),
            app_data: std::env::var_os("APPDATA"),
        }
    }

    fn absolute(value: &Option<OsString>) -> Option<PathBuf> {
        let path = PathBuf::from(value.as_ref()?);
        path.is_absolute().then_some(path)
    }

    fn home_relative(&self, tail: &[&str]) -> Option<PathBuf> {
        let mut path = Self::absolute(&self.home)?;
        path.extend(tail);
        Some(path)
    }

    fn state_base(&self) -> Option<PathBuf> {
        Self::absolute(&self.xdg_state_home)
            .or_else(|| self.home_relative(&[".local", "state"]))
            .or_else(|| Self::absolute(&self.local_app_data))
    }

    fn config_base(&self) -> Option<PathBuf> {
        Self::absolute(&self.xdg_config_home)
            .or_else(|| self.home_relative(&[".config"]))
            .or_else(|| Self::absolute(&self.app_data))
    }

    pub fn state_file(&self, server: &Url, endpoint: &str) -> Result<PathBuf, CliError> {
        let mut path = self.state_base().ok_or_else(|| {
            CliError::Usage(
                "cannot find a state directory (set HOME or XDG_STATE_HOME), or pass --state-file"
                    .to_owned(),
            )
        })?;
        path.push(APP_DIR);
        path.push(server_directory(server));
        path.push(format!("{endpoint}.json"));
        Ok(path)
    }

    pub fn token_file(&self) -> Result<PathBuf, CliError> {
        let mut path = self.config_base().ok_or_else(|| {
            CliError::Usage(
                "cannot find a config directory (set HOME or XDG_CONFIG_HOME)".to_owned(),
            )
        })?;
        path.push(APP_DIR);
        path.push(TOKEN_FILE);
        Ok(path)
    }
}

fn server_directory(server: &Url) -> String {
    let host = server.host_str().unwrap_or("unknown");
    let name = match server.port() {
        Some(port) => format!("{host}_{port}"),
        None => host.to_owned(),
    };
    name.chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn server(text: &str) -> Url {
        Url::parse(text).expect("valid url")
    }

    #[test]
    fn prefers_the_xdg_state_home_over_home() {
        let environment = Environment {
            xdg_state_home: Some("/xdg/state".into()),
            home: Some("/home/dev".into()),
            ..Environment::default()
        };
        let path = environment
            .state_file(&server("https://webhooks.live"), "abc")
            .expect("path");
        assert_eq!(
            path,
            PathBuf::from("/xdg/state/webhooks-live/webhooks.live/abc.json")
        );
    }

    #[test]
    fn falls_back_to_home_and_ignores_a_relative_xdg_value() {
        let environment = Environment {
            xdg_state_home: Some("relative/state".into()),
            home: Some("/home/dev".into()),
            ..Environment::default()
        };
        let path = environment
            .state_file(&server("http://127.0.0.1:8080"), "abc")
            .expect("path");
        assert_eq!(
            path,
            PathBuf::from("/home/dev/.local/state/webhooks-live/127.0.0.1_8080/abc.json")
        );
    }

    #[test]
    fn places_the_token_in_the_config_directory() {
        let environment = Environment {
            home: Some("/home/dev".into()),
            ..Environment::default()
        };
        assert_eq!(
            environment.token_file().expect("path"),
            PathBuf::from("/home/dev/.config/webhooks-live/token")
        );
    }

    #[test]
    fn reports_a_missing_home_as_a_usage_error() {
        let environment = Environment::default();
        assert!(matches!(environment.token_file(), Err(CliError::Usage(_))));
    }
}
