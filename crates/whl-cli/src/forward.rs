use std::time::{Duration, Instant};

use serde::de::DeserializeOwned;

use crate::api::{Api, ApiError};
use crate::backoff::Backoff;
use crate::error::CliError;
use crate::filter::Filters;
use crate::model::{CaptureSummary, Ready};
use crate::output::{Output, millis, safe};
use crate::replay::{Outcome, Replayer};
use crate::shutdown::Shutdown;
use crate::state::State;

enum End {
    Shutdown,
    Closed,
}

enum Flow {
    Continue,
    Stop,
    Reconnect,
}

enum Line {
    Always(String),
    Verbose(String),
}

pub struct Forwarder {
    pub api: Api,
    pub endpoint: String,
    pub replayer: Replayer,
    pub filters: Filters,
    pub state: State,
    pub output: Output,
    pub dry_run: bool,
    pub shutdown: Shutdown,
    pub cursor: Option<u64>,
    pub failing: Option<Failing>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Failing {
    pub seq: u64,
    pub attempts: u32,
}

pub const MAX_ATTEMPTS: u32 = 5;
pub const STABLE_AFTER: Duration = Duration::from_secs(30);

fn next_attempt(failing: Option<Failing>, seq: u64) -> u32 {
    match failing {
        Some(previous) if previous.seq == seq => previous.attempts + 1,
        _ => 1,
    }
}

fn earned_reset(progressed: bool, lived: Duration) -> bool {
    progressed || lived >= STABLE_AFTER
}

fn label(seq: u64, method: &str, path: &str, query: &str) -> String {
    let separator = if query.is_empty() { "" } else { "?" };
    format!(
        "#{seq} {} {}{separator}{}",
        safe(method),
        safe(path),
        safe(query)
    )
}

fn parse<T: DeserializeOwned>(name: &str, data: &str) -> Result<T, CliError> {
    serde_json::from_str(data)
        .map_err(|error| CliError::Api(ApiError::Malformed(format!("bad {name} event: {error}"))))
}

impl Forwarder {
    pub async fn run(mut self) -> Result<(), CliError> {
        let mut backoff = Backoff::new();
        let mut shutdown = self.shutdown.clone();
        loop {
            if self.shutdown.is_set() {
                return Ok(());
            }
            let before = self.cursor;
            let started = Instant::now();
            let result = self.session().await;
            let earned = earned_reset(self.cursor != before, started.elapsed());
            let floor = match result {
                Ok(End::Shutdown) => return Ok(()),
                Ok(End::Closed) => {
                    if earned {
                        backoff.reset();
                    }
                    self.output.notice("the stream ended")?;
                    None
                }
                Err(CliError::Api(error)) if !error.is_fatal() => {
                    if earned {
                        backoff.reset();
                    }
                    self.output.notice(&safe(&error.to_string()))?;
                    error.retry_after()
                }
                Err(error) => return Err(error),
            };
            let delay = backoff.next_delay(floor);
            self.output
                .notice(&format!("reconnecting in {:.1}s", delay.as_secs_f64()))?;
            tokio::select! {
                _ = tokio::time::sleep(delay) => {}
                _ = shutdown.wait() => return Ok(()),
            }
        }
    }

    async fn session(&mut self) -> Result<End, CliError> {
        let mut shutdown = self.shutdown.clone();
        let mut stream = tokio::select! {
            _ = shutdown.wait() => return Ok(End::Shutdown),
            opened = self.api.open_stream(self.cursor) => opened?,
        };
        loop {
            let event = tokio::select! {
                _ = shutdown.wait() => return Ok(End::Shutdown),
                event = stream.next_event() => event?,
            };
            let Some(event) = event else {
                return Ok(End::Closed);
            };
            let flow = match event.name.as_str() {
                "ready" => self.on_ready(parse("ready", &event.data)?).await?,
                "capture" => self.on_capture(parse("capture", &event.data)?).await?,
                "resync" => self.catch_up().await?,
                "endpoint_deleted" => {
                    return Err(CliError::Api(ApiError::Gone {
                        message: "the endpoint was deleted".to_owned(),
                    }));
                }
                "bye" => Flow::Reconnect,
                _ => Flow::Continue,
            };
            match flow {
                Flow::Continue => {}
                Flow::Stop => return Ok(End::Shutdown),
                Flow::Reconnect => return Ok(End::Closed),
            }
        }
    }

    async fn on_ready(&mut self, ready: Ready) -> Result<Flow, CliError> {
        if ready.endpoint_id != self.endpoint {
            return Err(CliError::Runtime(format!(
                "the server streams endpoint {} instead of {}",
                safe(&ready.endpoint_id),
                self.endpoint
            )));
        }
        match self.cursor {
            None => {
                self.output.notice(&format!(
                    "connected, forwarding captures after #{}",
                    ready.last_seq
                ))?;
                self.advance(ready.last_seq)?;
                Ok(Flow::Continue)
            }
            Some(cursor) => {
                self.output
                    .notice(&format!("connected, resuming after #{cursor}"))?;
                self.catch_up().await
            }
        }
    }

    async fn on_capture(&mut self, summary: CaptureSummary) -> Result<Flow, CliError> {
        let Some(cursor) = self.cursor else {
            return Err(CliError::Api(ApiError::Malformed(
                "a capture arrived before the ready event".to_owned(),
            )));
        };
        if summary.seq <= cursor {
            return Ok(Flow::Continue);
        }
        self.process(summary).await?;
        Ok(Flow::Continue)
    }

    async fn catch_up(&mut self) -> Result<Flow, CliError> {
        let after = self.cursor.unwrap_or(0);
        let mut previous = after;
        let mut page_cursor: Option<String> = None;
        loop {
            let page = self.api.list_after(after, page_cursor.as_deref()).await?;
            if page.items.is_empty() {
                return Ok(Flow::Continue);
            }
            for summary in page.items {
                if summary.seq <= previous {
                    return Err(CliError::Api(ApiError::Malformed(
                        "the capture list is not in ascending order".to_owned(),
                    )));
                }
                previous = summary.seq;
                self.process(summary).await?;
                if self.shutdown.is_set() {
                    return Ok(Flow::Stop);
                }
            }
            match page.next_cursor {
                None => return Ok(Flow::Continue),
                Some(next) if page_cursor.as_deref() == Some(next.as_str()) => {
                    return Err(CliError::Api(ApiError::Malformed(
                        "the server repeated a page cursor".to_owned(),
                    )));
                }
                Some(next) => page_cursor = Some(next),
            }
        }
    }

    fn advance(&mut self, seq: u64) -> Result<(), CliError> {
        if self.cursor.is_none_or(|cursor| seq > cursor) {
            self.state.record(seq)?;
            self.cursor = Some(seq);
        }
        Ok(())
    }

    async fn process(&mut self, summary: CaptureSummary) -> Result<(), CliError> {
        let line = match self.deliver(&summary).await {
            Ok(line) => {
                self.failing = None;
                line
            }
            Err(CliError::Api(error)) if !error.is_fatal() => {
                let attempts = next_attempt(self.failing, summary.seq);
                if attempts < MAX_ATTEMPTS {
                    self.failing = Some(Failing {
                        seq: summary.seq,
                        attempts,
                    });
                    return Err(CliError::Api(error));
                }
                self.failing = None;
                self.output.notice(&format!(
                    "#{}: giving up after {attempts} failed attempts: {}",
                    summary.seq,
                    safe(&error.to_string())
                ))?;
                Line::Always(format!(
                    "{} -> skipped: gave up after {attempts} failed attempts",
                    label(summary.seq, &summary.method, &summary.path, "")
                ))
            }
            Err(error) => return Err(error),
        };
        self.advance(summary.seq)?;
        match line {
            Line::Always(text) => self.output.event(&text)?,
            Line::Verbose(text) => self.output.detail(&text)?,
        }
        Ok(())
    }

    async fn deliver(&self, summary: &CaptureSummary) -> Result<Line, CliError> {
        let quick = label(summary.seq, &summary.method, &summary.path, "");
        if !summary.is_http() {
            let kind = summary.kind.as_deref().unwrap_or_default();
            return Ok(Line::Verbose(format!(
                "{quick} -> skipped: kind {}",
                safe(kind)
            )));
        }
        if !self.filters.matches(&summary.method, &summary.path) {
            return Ok(Line::Verbose(format!("{quick} -> skipped: filtered out")));
        }
        let detail = match self.api.detail(&summary.id).await {
            Ok(detail) => detail,
            Err(ApiError::CaptureNotFound) => return Ok(deleted(&quick)),
            Err(error) => return Err(error.into()),
        };
        let shown = label(
            summary.seq,
            &detail.method,
            &detail.path,
            &detail.query_string,
        );
        let plan = match self.replayer.plan(&detail) {
            Ok(plan) => plan,
            Err(error) => return Ok(Line::Always(format!("{shown} -> failed: {error}"))),
        };
        for dropped in &plan.dropped {
            self.output.notice(&format!(
                "#{}: header not replayed: {}",
                summary.seq,
                safe(dropped)
            ))?;
        }
        if self.dry_run {
            return Ok(Line::Always(format!(
                "{shown} -> dry run: would send {} {} ({} headers, {} body bytes)",
                plan.method,
                plan.url,
                plan.headers.len(),
                summary.body_size
            )));
        }
        let body = if summary.body_size == 0 {
            Vec::new()
        } else {
            match self.api.body(&summary.id).await {
                Ok(body) => body,
                Err(ApiError::CaptureNotFound) => return Ok(deleted(&quick)),
                Err(error) => return Err(error.into()),
            }
        };
        if body.len() as u64 != summary.body_size {
            self.output.notice(&format!(
                "#{}: the body route returned {} bytes but body_size is {}",
                summary.seq,
                body.len(),
                summary.body_size
            ))?;
        }
        let sent = self.replayer.send(plan, body).await;
        Ok(Line::Always(match sent.outcome {
            Outcome::Delivered { status } => {
                format!("{shown} -> {status} {}", millis(sent.elapsed))
            }
            Outcome::Failed { reason } => format!(
                "{shown} -> failed: {} ({})",
                safe(&reason),
                millis(sent.elapsed)
            ),
        }))
    }
}

fn deleted(quick: &str) -> Line {
    Line::Always(format!(
        "{quick} -> skipped: deleted before it could be forwarded"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_attempts_per_capture_and_restarts_for_a_new_one() {
        assert_eq!(next_attempt(None, 7), 1);
        let second = Failing {
            seq: 7,
            attempts: 1,
        };
        assert_eq!(next_attempt(Some(second), 7), 2);
        assert_eq!(next_attempt(Some(second), 8), 1);
    }

    #[test]
    fn resets_the_backoff_only_after_progress_or_a_stable_stream() {
        assert!(!earned_reset(false, Duration::from_secs(1)));
        assert!(earned_reset(true, Duration::from_secs(1)));
        assert!(earned_reset(false, STABLE_AFTER));
    }
}
