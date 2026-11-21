use std::io;
use std::time::Duration;

use bytes::{Buf, BytesMut};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::watch;
use tokio::time::Instant;

use crate::data::{DataCollector, Feed};
use crate::gate::{MemoryBudget, Reservation};

const SCRATCH_BYTES: usize = 4096;
const CLOSE_TIMEOUT: Duration = Duration::from_secs(2);

#[derive(Debug)]
pub enum LineFailure {
    Closed,
    TooLong,
    Timeout,
    Stopped,
    Io(io::Error),
}

#[derive(Debug)]
pub enum DataFailure {
    Closed,
    TooLarge,
    Timeout,
    Stopped,
    OutOfMemory,
    Io(io::Error),
}

#[derive(Debug)]
pub struct Collected {
    pub data: Vec<u8>,
    pub reservation: Reservation,
}

pub async fn until_stopped(stop: &mut watch::Receiver<bool>) {
    let _ = stop.wait_for(|stopped| *stopped).await;
}

pub struct Wire<S> {
    stream: S,
    buf: BytesMut,
    stop: watch::Receiver<bool>,
}

impl<S: AsyncRead + AsyncWrite + Unpin> Wire<S> {
    pub fn new(stream: S, stop: watch::Receiver<bool>) -> Self {
        Self {
            stream,
            buf: BytesMut::new(),
            stop,
        }
    }

    pub fn buffered(&self) -> usize {
        self.buf.len()
    }

    pub fn into_stream(self) -> S {
        self.stream
    }

    async fn fill(&mut self, deadline: Instant, scratch: &mut [u8]) -> Result<usize, LineFailure> {
        let read = self.stream.read(scratch);
        let stopped = until_stopped(&mut self.stop);
        tokio::select! {
            biased;
            _ = stopped => Err(LineFailure::Stopped),
            outcome = tokio::time::timeout_at(deadline, read) => match outcome {
                Err(_) => Err(LineFailure::Timeout),
                Ok(Err(error)) => Err(LineFailure::Io(error)),
                Ok(Ok(0)) => Err(LineFailure::Closed),
                Ok(Ok(count)) => Ok(count),
            },
        }
    }

    pub async fn read_line(
        &mut self,
        max_line: usize,
        deadline: Instant,
    ) -> Result<BytesMut, LineFailure> {
        let mut scratch = [0_u8; SCRATCH_BYTES];
        loop {
            if let Some(position) = self.buf.iter().position(|byte| *byte == b'\n') {
                if position + 1 > max_line {
                    return Err(LineFailure::TooLong);
                }
                return Ok(self.buf.split_to(position + 1));
            }
            if self.buf.len() >= max_line {
                return Err(LineFailure::TooLong);
            }
            let count = self.fill(deadline, &mut scratch).await?;
            self.buf.extend_from_slice(&scratch[..count]);
        }
    }

    pub async fn read_data(
        &mut self,
        max_bytes: usize,
        deadline: Instant,
        memory: &MemoryBudget,
    ) -> Result<Collected, DataFailure> {
        let mut collector = DataCollector::new(max_bytes);
        let mut reservation = memory.reservation();
        let mut scratch = [0_u8; SCRATCH_BYTES];
        let mut pending = self.buf.split();
        loop {
            if !pending.is_empty() {
                if !reservation.ensure(collector.len() + pending.len()) {
                    return Err(DataFailure::OutOfMemory);
                }
                match collector.feed(&pending) {
                    Feed::NeedMore => pending.clear(),
                    Feed::TooLarge => return Err(DataFailure::TooLarge),
                    Feed::Done { consumed } => {
                        pending.advance(consumed);
                        self.buf = pending;
                        return Ok(Collected {
                            data: collector.into_bytes(),
                            reservation,
                        });
                    }
                }
            }
            let count =
                self.fill(deadline, &mut scratch)
                    .await
                    .map_err(|failure| match failure {
                        LineFailure::Closed => DataFailure::Closed,
                        LineFailure::Timeout => DataFailure::Timeout,
                        LineFailure::Stopped => DataFailure::Stopped,
                        LineFailure::Io(error) => DataFailure::Io(error),
                        LineFailure::TooLong => DataFailure::TooLarge,
                    })?;
            pending.extend_from_slice(&scratch[..count]);
        }
    }

    pub async fn close(&mut self) {
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, self.stream.shutdown()).await;
    }

    pub async fn write(&mut self, bytes: &[u8], timeout: Duration) -> io::Result<()> {
        let write = async {
            self.stream.write_all(bytes).await?;
            self.stream.flush().await
        };
        tokio::time::timeout(timeout, write).await.map_err(|_| {
            io::Error::new(io::ErrorKind::TimedOut, "the peer did not read the reply")
        })?
    }
}
