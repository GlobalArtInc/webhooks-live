use std::future::Future;
use std::io::{self, IoSlice};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::time::Sleep;

pub const WRITE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug)]
pub struct WriteTimeout<S> {
    inner: S,
    limit: Duration,
    stalled: Option<Pin<Box<Sleep>>>,
}

impl<S> WriteTimeout<S> {
    pub fn new(inner: S, limit: Duration) -> Self {
        Self {
            inner,
            limit,
            stalled: None,
        }
    }

    pub fn into_inner(self) -> S {
        self.inner
    }

    fn stall(&mut self, cx: &mut Context<'_>) -> io::Result<()> {
        let limit = self.limit;
        let timer = self
            .stalled
            .get_or_insert_with(|| Box::pin(tokio::time::sleep(limit)));
        match timer.as_mut().poll(cx) {
            Poll::Ready(()) => Err(io::Error::new(
                io::ErrorKind::TimedOut,
                "the peer did not accept data within the write timeout",
            )),
            Poll::Pending => Ok(()),
        }
    }

    fn settle<T>(
        &mut self,
        cx: &mut Context<'_>,
        polled: Poll<io::Result<T>>,
        resets: bool,
    ) -> Poll<io::Result<T>> {
        match polled {
            Poll::Ready(result) => {
                if resets {
                    self.stalled = None;
                }
                Poll::Ready(result)
            }
            Poll::Pending => match self.stall(cx) {
                Ok(()) => Poll::Pending,
                Err(error) => Poll::Ready(Err(error)),
            },
        }
    }
}

impl<S: AsyncRead + Unpin> AsyncRead for WriteTimeout<S> {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(cx, buf)
    }
}

impl<S: AsyncWrite + Unpin> AsyncWrite for WriteTimeout<S> {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write(cx, buf);
        self.settle(cx, polled, true)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        let polled = Pin::new(&mut self.inner).poll_write_vectored(cx, bufs);
        self.settle(cx, polled, true)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let polled = Pin::new(&mut self.inner).poll_flush(cx);
        self.settle(cx, polled, false)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let polled = Pin::new(&mut self.inner).poll_shutdown(cx);
        self.settle(cx, polled, false)
    }
}
