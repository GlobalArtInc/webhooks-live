use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::fsio;

const STATE_VERSION: u32 = 1;
const MAX_STATE_BYTES: u64 = 4096;
const LOCK_SUFFIX: &str = ".lock";

#[derive(Debug, thiserror::Error)]
pub enum StateError {
    #[error("cannot read the state file {path}: {source}")]
    Read { path: PathBuf, source: io::Error },
    #[error("cannot write the state file {path}: {source}")]
    Write { path: PathBuf, source: io::Error },
    #[error("the state file {path} is not valid ({reason}); fix or delete it")]
    Corrupt { path: PathBuf, reason: String },
    #[error(
        "the state file {path} belongs to endpoint {found_endpoint} on {found_server}, not to {endpoint} on {server}"
    )]
    Mismatch {
        path: PathBuf,
        found_server: String,
        found_endpoint: String,
        server: String,
        endpoint: String,
    },
    #[error("another forwarder already uses the state file {0}")]
    Locked(PathBuf),
    #[error("cannot lock the state file {path}: {source}")]
    Lock { path: PathBuf, source: io::Error },
}

#[derive(Debug, Serialize, Deserialize)]
struct StateRecord {
    version: u32,
    server: String,
    endpoint_id: String,
    last_seq: u64,
}

#[derive(Debug)]
pub struct State {
    path: PathBuf,
    server: String,
    endpoint: String,
    last_seq: Option<u64>,
    persist: bool,
    _lock: Option<File>,
}

impl State {
    pub fn open(
        path: PathBuf,
        server: &str,
        endpoint: &str,
        persist: bool,
    ) -> Result<Self, StateError> {
        let lock = if persist {
            Some(acquire_lock(&path)?)
        } else {
            None
        };
        let last_seq = load(&path, server, endpoint)?;
        Ok(Self {
            path,
            server: server.to_owned(),
            endpoint: endpoint.to_owned(),
            last_seq,
            persist,
            _lock: lock,
        })
    }

    pub fn last_seq(&self) -> Option<u64> {
        self.last_seq
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn record(&mut self, seq: u64) -> Result<(), StateError> {
        if self.persist {
            let record = StateRecord {
                version: STATE_VERSION,
                server: self.server.clone(),
                endpoint_id: self.endpoint.clone(),
                last_seq: seq,
            };
            let bytes = serde_json::to_vec(&record).map_err(|error| StateError::Write {
                path: self.path.clone(),
                source: io::Error::other(error),
            })?;
            fsio::write_private_atomic(&self.path, &bytes).map_err(|source| StateError::Write {
                path: self.path.clone(),
                source,
            })?;
        }
        self.last_seq = Some(seq);
        Ok(())
    }
}

fn acquire_lock(path: &Path) -> Result<File, StateError> {
    let mut name = path
        .file_name()
        .ok_or_else(|| StateError::Lock {
            path: path.to_owned(),
            source: io::Error::new(io::ErrorKind::InvalidInput, "the path has no file name"),
        })?
        .to_os_string();
    name.push(LOCK_SUFFIX);
    let lock_path = path.with_file_name(name);
    let lock_error = |source| StateError::Lock {
        path: lock_path.clone(),
        source,
    };
    if let Some(directory) = lock_path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fsio::create_private_dir(directory).map_err(lock_error)?;
    }
    let file = fsio::open_lock_file(&lock_path).map_err(lock_error)?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(StateError::Locked(path.to_owned())),
        Err(std::fs::TryLockError::Error(source)) => Err(lock_error(source)),
    }
}

fn load(path: &Path, server: &str, endpoint: &str) -> Result<Option<u64>, StateError> {
    let bytes = match fsio::read_limited(path, MAX_STATE_BYTES) {
        Ok(Some(bytes)) => bytes,
        Ok(None) => return Ok(None),
        Err(source) if source.kind() == io::ErrorKind::InvalidData => {
            return Err(StateError::Corrupt {
                path: path.to_owned(),
                reason: source.to_string(),
            });
        }
        Err(source) => {
            return Err(StateError::Read {
                path: path.to_owned(),
                source,
            });
        }
    };
    let record: StateRecord =
        serde_json::from_slice(&bytes).map_err(|error| StateError::Corrupt {
            path: path.to_owned(),
            reason: error.to_string(),
        })?;
    if record.version != STATE_VERSION {
        return Err(StateError::Corrupt {
            path: path.to_owned(),
            reason: format!("unsupported version {}", record.version),
        });
    }
    if record.server != server || record.endpoint_id != endpoint {
        return Err(StateError::Mismatch {
            path: path.to_owned(),
            found_server: record.server,
            found_endpoint: record.endpoint_id,
            server: server.to_owned(),
            endpoint: endpoint.to_owned(),
        });
    }
    Ok(Some(record.last_seq))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU32, Ordering};

    use super::*;

    const SERVER: &str = "https://webhooks.live";
    const ENDPOINT: &str = "7k2m9q4xw8c1d5f3h6j0n2p4";

    static NEXT: AtomicU32 = AtomicU32::new(0);

    struct Scratch(PathBuf);

    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "whl-cli-state-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            Self(path)
        }

        fn file(&self) -> PathBuf {
            self.0.join("nested").join("endpoint.json")
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn starts_empty_and_remembers_the_last_recorded_seq() {
        let scratch = Scratch::new();
        let mut state = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("opens");
        assert_eq!(state.last_seq(), None);
        state.record(41).expect("records");
        state.record(42).expect("records");
        drop(state);
        let reopened = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("reopens");
        assert_eq!(reopened.last_seq(), Some(42));
    }

    #[test]
    fn leaves_no_temporary_file_behind() {
        let scratch = Scratch::new();
        let mut state = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("opens");
        state.record(1).expect("records");
        let names: Vec<String> = std::fs::read_dir(scratch.0.join("nested"))
            .expect("lists")
            .map(|entry| {
                entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert!(
            names.iter().all(|name| !name.ends_with(".tmp")),
            "{names:?}"
        );
    }

    #[test]
    fn refuses_a_state_file_of_another_endpoint() {
        let scratch = Scratch::new();
        let mut state = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("opens");
        state.record(5).expect("records");
        drop(state);
        let other = State::open(scratch.file(), SERVER, "aaaaaaaaaaaaaaaaaaaaaaaa", true);
        assert!(matches!(other, Err(StateError::Mismatch { .. })));
    }

    #[test]
    fn refuses_a_corrupt_state_file_instead_of_starting_over() {
        let scratch = Scratch::new();
        std::fs::create_dir_all(scratch.0.join("nested")).expect("mkdir");
        std::fs::write(scratch.file(), b"{not json").expect("writes");
        let state = State::open(scratch.file(), SERVER, ENDPOINT, true);
        assert!(matches!(state, Err(StateError::Corrupt { .. })));
    }

    #[test]
    fn refuses_a_second_forwarder_on_the_same_state_file() {
        let scratch = Scratch::new();
        let first = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("opens");
        let second = State::open(scratch.file(), SERVER, ENDPOINT, true);
        assert!(matches!(second, Err(StateError::Locked(_))));
        drop(first);
    }

    #[cfg(unix)]
    #[test]
    fn refuses_a_lock_file_that_is_a_symbolic_link() {
        let scratch = Scratch::new();
        std::fs::create_dir_all(scratch.0.join("nested")).expect("mkdir");
        let victim = scratch.0.join("victim");
        std::fs::write(&victim, b"keep").expect("writes");
        std::os::unix::fs::symlink(&victim, scratch.0.join("nested").join("endpoint.json.lock"))
            .expect("symlink");
        let state = State::open(scratch.file(), SERVER, ENDPOINT, true);
        assert!(matches!(state, Err(StateError::Lock { .. })));
        assert_eq!(std::fs::read(&victim).expect("reads"), b"keep");
    }

    #[test]
    fn writes_nothing_when_persistence_is_off() {
        let scratch = Scratch::new();
        let mut state = State::open(scratch.file(), SERVER, ENDPOINT, false).expect("opens");
        state.record(9).expect("records in memory");
        assert_eq!(state.last_seq(), Some(9));
        assert!(!scratch.file().exists());
    }

    #[cfg(unix)]
    #[test]
    fn keeps_the_state_file_private() {
        use std::os::unix::fs::PermissionsExt;

        let scratch = Scratch::new();
        let mut state = State::open(scratch.file(), SERVER, ENDPOINT, true).expect("opens");
        state.record(1).expect("records");
        let mode = std::fs::metadata(scratch.file())
            .expect("metadata")
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}
