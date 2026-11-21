use std::io::{self, BufRead, Read};
use std::path::Path;

use crate::error::CliError;
use crate::fsio;
use crate::model::Token;

const MAX_TOKEN_FILE_BYTES: u64 = 1024;

pub fn read_token(mut reader: impl BufRead) -> Result<Token, CliError> {
    let mut line = String::new();
    let read = reader
        .by_ref()
        .take(MAX_TOKEN_FILE_BYTES)
        .read_line(&mut line)
        .map_err(|error| CliError::Usage(format!("cannot read the token from stdin: {error}")))?;
    if read == 0 {
        return Err(CliError::Usage("no token on stdin".to_owned()));
    }
    line.trim()
        .parse()
        .map_err(|error| CliError::Usage(format!("the token from stdin is refused: {error}")))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredToken {
    pub server: String,
    pub token: Token,
}

pub fn load_token(path: &Path) -> Result<Option<StoredToken>, CliError> {
    let bytes = match fsio::read_limited(path, MAX_TOKEN_FILE_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(error) => {
            return Err(CliError::Usage(format!(
                "cannot read the stored token {}: {error}",
                path.display()
            )));
        }
    };
    let refused = |reason: &str| {
        CliError::Usage(format!(
            "the stored token {} is refused: {reason}; run login again",
            path.display()
        ))
    };
    let text = String::from_utf8(bytes).map_err(|_| refused("not valid text"))?;
    let mut lines = text.lines();
    let server = lines.next().map(str::trim).filter(|line| !line.is_empty());
    let token = lines.next().map(str::trim);
    match (server, token) {
        (Some(server), Some(token)) => Ok(Some(StoredToken {
            server: server.to_owned(),
            token: token
                .parse()
                .map_err(|error| refused(&format!("{error}")))?,
        })),
        _ => Err(refused("expected a server line and a token line")),
    }
}

pub fn store_token(path: &Path, server: &str, token: &Token) -> Result<(), CliError> {
    let contents = format!("{server}\n{}\n", token.unpack());
    fsio::write_private_atomic(path, contents.as_bytes()).map_err(|error| {
        CliError::Runtime(format!(
            "cannot store the token in {}: {error}",
            path.display()
        ))
    })
}

pub fn remove_token(path: &Path) -> Result<bool, CliError> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(CliError::Runtime(format!(
            "cannot remove the stored token {}: {error}",
            path.display()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn scratch(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("whl-cli-auth-{}-{name}", std::process::id()))
    }

    #[test]
    fn reads_the_first_line_of_stdin_as_the_token() {
        let token = read_token(&b"whl_abc123\nignored\n"[..]).expect("token");
        assert_eq!(token.unpack(), "whl_abc123");
    }

    #[test]
    fn refuses_an_empty_stdin() {
        assert!(matches!(read_token(&b""[..]), Err(CliError::Usage(_))));
    }

    #[test]
    fn stores_and_removes_a_token_privately() {
        let path = scratch("roundtrip").join("token");
        let token: Token = "whl_roundtrip".parse().expect("token");
        store_token(&path, "https://webhooks.live", &token).expect("stores");
        assert_eq!(
            load_token(&path).expect("loads"),
            Some(StoredToken {
                server: "https://webhooks.live".to_owned(),
                token,
            })
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(remove_token(&path).expect("removes"));
        assert!(!remove_token(&path).expect("second removal"));
        let _ = std::fs::remove_dir_all(path.parent().expect("parent"));
    }
}
