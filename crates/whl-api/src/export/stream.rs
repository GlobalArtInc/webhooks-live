use std::error::Error;
use std::future::Future;
use std::io;
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use axum::body::{Body, Bytes};
use futures_util::stream::{self as chunked, Stream};
use time::OffsetDateTime;
use tokio::io::{AsyncReadExt, AsyncWrite, AsyncWriteExt, DuplexStream};
use tokio::sync::oneshot;
use tokio::time::Sleep;
use whl_core::capture_id::CaptureId;
use whl_export::{Archive, Encoder, ExportError, ExportFormat, body_entry_name, index_line};
use whl_store::{
    CaptureDetail, CaptureError, EndpointRecord, ExportSelection, ExportSnapshot, Store,
};

use super::slots::ExportPermit;

const PAGE: i64 = 50;
const CHUNK_BYTES: usize = 64 * 1024;
const PIPE_BYTES: usize = 4 * CHUNK_BYTES;
pub const MAX_EXPORT_BYTES: u64 = 256 * 1024 * 1024;
const OUTPUT_CEILING_BYTES: u64 = 2 * MAX_EXPORT_BYTES;
const WRITE_IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_EXPORT_DURATION: Duration = Duration::from_secs(10 * 60);

#[derive(Debug, Clone, Copy)]
struct Limits {
    idle: Duration,
    total: Duration,
    output_bytes: u64,
}

impl Limits {
    const STANDARD: Self = Self {
        idle: WRITE_IDLE_TIMEOUT,
        total: MAX_EXPORT_DURATION,
        output_bytes: OUTPUT_CEILING_BYTES,
    };
}

pub struct Job {
    pub store: Store,
    pub endpoint: EndpointRecord,
    pub selection: ExportSelection,
    pub snapshot: ExportSnapshot,
    pub format: ExportFormat,
    pub exported_at: OffsetDateTime,
    pub _permit: ExportPermit,
}

#[derive(Debug, thiserror::Error)]
enum StreamFailure {
    #[error("the capture store failed: {0}")]
    Store(#[from] CaptureError),
    #[error("the export cannot be written: {0}")]
    Export(#[from] ExportError),
    #[error("the export pipe failed: {0}")]
    Pipe(#[from] io::Error),
    #[error("the export ran longer than its time limit")]
    Deadline,
}

impl StreamFailure {
    fn client_caused(&self) -> bool {
        match self {
            Self::Store(_) | Self::Deadline => false,
            Self::Export(error) => io_kind_is_client_side(error),
            Self::Pipe(error) => io_kind_is_client_side(error),
        }
    }
}

fn io_kind_is_client_side(error: &(dyn Error + 'static)) -> bool {
    let mut current = Some(error);
    while let Some(candidate) = current {
        if let Some(io_error) = candidate.downcast_ref::<io::Error>() {
            return matches!(
                io_error.kind(),
                io::ErrorKind::BrokenPipe
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::TimedOut
            );
        }
        current = candidate.source();
    }
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Complete,
    Failed,
}

struct PipeWriter {
    inner: DuplexStream,
    idle: Duration,
    remaining: u64,
    blocked: Option<Pin<Box<Sleep>>>,
}

impl PipeWriter {
    fn new(inner: DuplexStream, limits: Limits) -> Self {
        Self {
            inner,
            idle: limits.idle,
            remaining: limits.output_bytes,
            blocked: None,
        }
    }

    fn settle<T>(
        &mut self,
        cx: &mut Context<'_>,
        polled: Poll<io::Result<T>>,
    ) -> Poll<io::Result<T>> {
        match polled {
            Poll::Ready(result) => {
                self.blocked = None;
                Poll::Ready(result)
            }
            Poll::Pending => {
                let idle = self.idle;
                let sleep = self
                    .blocked
                    .get_or_insert_with(|| Box::pin(tokio::time::sleep(idle)));
                match sleep.as_mut().poll(cx) {
                    Poll::Ready(()) => {
                        self.blocked = None;
                        Poll::Ready(Err(io::Error::new(
                            io::ErrorKind::TimedOut,
                            "the client stopped reading the export",
                        )))
                    }
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}

impl AsyncWrite for PipeWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let wanted = u64::try_from(buf.len()).unwrap_or(u64::MAX);
        if wanted > this.remaining {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::FileTooLarge,
                "the export is larger than its output ceiling",
            )));
        }
        let polled = Pin::new(&mut this.inner).poll_write(cx, buf);
        if let Poll::Ready(Ok(written)) = &polled {
            this.remaining = this
                .remaining
                .saturating_sub(u64::try_from(*written).unwrap_or(u64::MAX));
        }
        this.settle(cx, polled)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_flush(cx);
        this.settle(cx, polled)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        let polled = Pin::new(&mut this.inner).poll_shutdown(cx);
        this.settle(cx, polled)
    }
}

pub fn body(job: Job) -> Body {
    pipe(Limits::STANDARD, move |writer| produce(job, writer))
}

fn pipe<P, F>(limits: Limits, producer: P) -> Body
where
    P: FnOnce(PipeWriter) -> F + Send + 'static,
    F: Future<Output = Result<(), StreamFailure>> + Send + 'static,
{
    let (writer, reader) = tokio::io::duplex(PIPE_BYTES);
    let (done, finished) = oneshot::channel::<Outcome>();
    tokio::spawn(async move {
        let run = producer(PipeWriter::new(writer, limits));
        let result = match tokio::time::timeout(limits.total, run).await {
            Ok(result) => result,
            Err(_) => Err(StreamFailure::Deadline),
        };
        let outcome = match result {
            Ok(()) => Outcome::Complete,
            Err(error) => {
                if error.client_caused() {
                    tracing::debug!(%error, "an export client stopped reading");
                } else {
                    tracing::warn!(%error, "an export stopped before its end");
                }
                Outcome::Failed
            }
        };
        if done.send(outcome).is_err() {
            tracing::debug!("an export client left before the outcome was read");
        }
    });
    Body::from_stream(chunks(reader, finished))
}

fn chunks(
    reader: DuplexStream,
    finished: oneshot::Receiver<Outcome>,
) -> impl Stream<Item = Result<Bytes, io::Error>> + Send + 'static {
    chunked::unfold(Some((reader, finished)), |state| async move {
        let (mut reader, finished) = state?;
        let mut buffer = vec![0u8; CHUNK_BYTES];
        match reader.read(&mut buffer).await {
            Ok(0) => match finished.await {
                Ok(Outcome::Complete) => None,
                Ok(Outcome::Failed) => Some((
                    Err(io::Error::other("the export stopped before its end")),
                    None,
                )),
                Err(_) => Some((
                    Err(io::Error::other("the export task ended without a result")),
                    None,
                )),
            },
            Ok(read) => Some((
                Ok(Bytes::copy_from_slice(&buffer[..read])),
                Some((reader, finished)),
            )),
            Err(error) => Some((Err(error), None)),
        }
    })
}

async fn produce(job: Job, writer: PipeWriter) -> Result<(), StreamFailure> {
    match job.format {
        ExportFormat::Zip => write_archive(&job, writer).await,
        format => write_text(&job, format, writer).await,
    }
}

async fn write_text(
    job: &Job,
    format: ExportFormat,
    mut writer: PipeWriter,
) -> Result<(), StreamFailure> {
    let mut encoder = Encoder::new(format);
    writer
        .write_all(&encoder.header(&job.endpoint, job.exported_at)?)
        .await?;
    let mut cursor = 0;
    loop {
        let page = page(job, cursor).await?;
        let Some(last) = page.last() else {
            break;
        };
        cursor = last.summary.seq;
        for detail in &page {
            if let Some(body) = body_of(job, detail).await? {
                writer.write_all(&encoder.capture(detail, &body)?).await?;
            }
        }
    }
    writer.write_all(&encoder.footer()).await?;
    Ok(())
}

async fn write_archive(job: &Job, writer: PipeWriter) -> Result<(), StreamFailure> {
    let mut archive = Archive::new(writer);
    let mut cursor = 0;
    loop {
        let page = page(job, cursor).await?;
        let Some(last) = page.last() else {
            break;
        };
        cursor = last.summary.seq;
        for detail in &page {
            if let Some(body) = body_of(job, detail).await? {
                let name = body_entry_name(detail.summary.seq, &detail.summary.id);
                archive.put_body(&name, &body).await?;
            }
        }
    }
    let mut index = archive.open_index().await?;
    let mut cursor = 0;
    loop {
        let page = page(job, cursor).await?;
        let Some(last) = page.last() else {
            break;
        };
        cursor = last.summary.seq;
        for detail in &page {
            index.push(&index_line(detail)?).await?;
        }
    }
    index.finish().await?;
    archive.close().await?;
    Ok(())
}

async fn page(job: &Job, cursor: i64) -> Result<Vec<CaptureDetail>, CaptureError> {
    job.store
        .export_page(
            &job.endpoint.id,
            &job.selection,
            cursor,
            job.snapshot.last_seq,
            PAGE,
        )
        .await
}

async fn body_of(job: &Job, detail: &CaptureDetail) -> Result<Option<Vec<u8>>, CaptureError> {
    let capture = CaptureId::parse(&detail.summary.id).map_err(|_| CaptureError::Invalid {
        reason: format!("capture {} has an invalid id", detail.summary.id),
    })?;
    job.store.capture_body(&job.endpoint.id, &capture).await
}

#[cfg(test)]
mod tests {
    use std::net::{IpAddr, Ipv4Addr};

    use whl_auth::Caller;

    use super::*;
    use crate::export::slots::ExportSlots;

    const LIMITS: Limits = Limits {
        idle: Duration::from_secs(30),
        total: Duration::from_secs(600),
        output_bytes: u64::MAX,
    };

    fn caller() -> Caller {
        Caller::Anonymous {
            ip: Some(IpAddr::V4(Ipv4Addr::new(203, 0, 113, 7))),
        }
    }

    async fn drain(body: Body) -> Result<Bytes, axum::Error> {
        axum::body::to_bytes(body, usize::MAX).await
    }

    #[tokio::test]
    async fn a_producer_that_ends_cleanly_ends_the_body_cleanly() {
        let body = pipe(LIMITS, |mut writer| async move {
            writer.write_all(b"complete").await?;
            Ok(())
        });
        assert_eq!(drain(body).await.expect("the body ends"), "complete");
    }

    #[tokio::test]
    async fn a_producer_error_aborts_the_body_after_the_bytes_already_written() {
        let body = pipe(LIMITS, |mut writer| async move {
            writer.write_all(b"partial").await?;
            Err(StreamFailure::Pipe(io::Error::other("the store failed")))
        });
        assert!(drain(body).await.is_err());
    }

    async fn panicking(_writer: PipeWriter) -> Result<(), StreamFailure> {
        panic!("the producer panics");
    }

    #[tokio::test]
    async fn a_producer_panic_aborts_the_body() {
        let body = pipe(LIMITS, panicking);
        assert!(drain(body).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn a_reader_that_stalls_loses_the_slot_after_the_idle_timeout() {
        let slots = ExportSlots::new(1);
        let permit = slots.acquire(&caller()).expect("the slot is free");
        let body = pipe(LIMITS, move |mut writer| async move {
            let _permit = permit;
            writer.write_all(&vec![7u8; PIPE_BYTES * 4]).await?;
            Ok(())
        });
        tokio::time::sleep(Duration::from_secs(29)).await;
        assert!(slots.acquire(&caller()).is_none());
        tokio::time::sleep(Duration::from_secs(2)).await;
        assert!(slots.acquire(&caller()).is_some());
        assert!(drain(body).await.is_err());
    }

    #[tokio::test(start_paused = true)]
    async fn an_export_that_outlives_the_time_limit_is_aborted_and_frees_the_slot() {
        let slots = ExportSlots::new(1);
        let permit = slots.acquire(&caller()).expect("the slot is free");
        let body = pipe(LIMITS, move |_writer| async move {
            let _permit = permit;
            std::future::pending::<Result<(), StreamFailure>>().await
        });
        tokio::time::sleep(LIMITS.total + Duration::from_secs(1)).await;
        assert!(slots.acquire(&caller()).is_some());
        assert!(drain(body).await.is_err());
    }

    #[tokio::test]
    async fn output_beyond_the_ceiling_is_refused() {
        let (inner, _reader) = tokio::io::duplex(PIPE_BYTES);
        let mut writer = PipeWriter::new(
            inner,
            Limits {
                output_bytes: 10,
                ..LIMITS
            },
        );
        writer
            .write_all(b"0123456789")
            .await
            .expect("ten bytes fit");
        let error = writer
            .write_all(b"x")
            .await
            .expect_err("the eleventh byte is refused");
        assert_eq!(error.kind(), io::ErrorKind::FileTooLarge);
    }

    #[test]
    fn only_pipe_level_failures_count_as_the_client_leaving() {
        let gone = StreamFailure::Pipe(io::Error::from(io::ErrorKind::BrokenPipe));
        let stalled = StreamFailure::Pipe(io::Error::from(io::ErrorKind::TimedOut));
        let too_large = StreamFailure::Pipe(io::Error::from(io::ErrorKind::FileTooLarge));
        assert!(gone.client_caused());
        assert!(stalled.client_caused());
        assert!(!too_large.client_caused());
        assert!(!StreamFailure::Deadline.client_caused());
    }
}
