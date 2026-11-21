use std::sync::OnceLock;

use metrics_exporter_prometheus::{PrometheusBuilder, PrometheusHandle};

use crate::TelemetryError;

static RECORDER: OnceLock<Result<PrometheusHandle, String>> = OnceLock::new();

pub fn install() -> Result<(), TelemetryError> {
    let installed = RECORDER.get_or_init(|| {
        PrometheusBuilder::new()
            .install_recorder()
            .map_err(|error| error.to_string())
    });
    installed
        .as_ref()
        .map(|_| ())
        .map_err(|message| TelemetryError::Metrics(message.clone()))
}

pub fn render() -> String {
    match RECORDER.get() {
        Some(Ok(handle)) => handle.render(),
        _ => String::new(),
    }
}
