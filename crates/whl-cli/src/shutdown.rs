use std::io;

use tokio::sync::watch;

use crate::error::CliError;
use crate::output::Output;

const INTERRUPTED_EXIT_CODE: i32 = 130;

#[derive(Debug, Clone)]
pub struct Shutdown {
    receiver: watch::Receiver<bool>,
}

impl Shutdown {
    pub fn channel() -> (watch::Sender<bool>, Self) {
        let (sender, receiver) = watch::channel(false);
        (sender, Self { receiver })
    }

    pub fn is_set(&self) -> bool {
        *self.receiver.borrow()
    }

    pub async fn wait(&mut self) {
        loop {
            if *self.receiver.borrow_and_update() {
                return;
            }
            if self.receiver.changed().await.is_err() {
                std::future::pending::<()>().await;
            }
        }
    }
}

#[cfg(unix)]
struct Signals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl Signals {
    fn new() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }

    async fn next(&mut self) {
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
    }
}

#[cfg(not(unix))]
struct Signals;

#[cfg(not(unix))]
impl Signals {
    fn new() -> io::Result<Self> {
        Ok(Self)
    }

    async fn next(&mut self) {
        let _ = tokio::signal::ctrl_c().await;
    }
}

pub fn install(output: Output) -> Result<Shutdown, CliError> {
    let mut signals = Signals::new()
        .map_err(|error| CliError::Runtime(format!("cannot listen for signals: {error}")))?;
    let (sender, shutdown) = Shutdown::channel();
    tokio::spawn(async move {
        signals.next().await;
        let _ = sender.send(true);
        let _ =
            output.notice("stopping after the request in flight; press Ctrl-C again to quit now");
        signals.next().await;
        std::process::exit(INTERRUPTED_EXIT_CODE);
    });
    Ok(shutdown)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn reports_a_shutdown_to_every_waiter() {
        let (sender, shutdown) = Shutdown::channel();
        let mut waiter = shutdown.clone();
        assert!(!shutdown.is_set());
        sender.send(true).expect("receiver alive");
        waiter.wait().await;
        assert!(shutdown.is_set());
    }
}
