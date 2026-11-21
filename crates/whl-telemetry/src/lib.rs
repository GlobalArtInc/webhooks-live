pub mod logs;
pub mod metrics;
pub mod series;

#[derive(Debug, thiserror::Error)]
pub enum TelemetryError {
    #[error("invalid log filter: {0}")]
    Filter(#[from] tracing_subscriber::filter::ParseError),
    #[error("cannot install the log subscriber: {0}")]
    Subscriber(#[from] tracing_subscriber::util::TryInitError),
    #[error("cannot install the metrics recorder: {0}")]
    Metrics(String),
}
