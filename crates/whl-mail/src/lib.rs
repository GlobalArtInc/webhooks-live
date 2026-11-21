mod data;
mod deliver;
mod error;
mod gate;
pub mod message;
mod recipient;
mod server;
mod session;
mod settings;
mod shared;
mod tls;
mod wire;

pub use error::{MailSetupError, TlsError};
pub use recipient::{MAX_TAG_BYTES, Recipient, parse_recipient};
pub use server::MailServer;
pub use settings::{MailDomain, MailLimits, MailSettings, StorageProbe, TlsFiles};
