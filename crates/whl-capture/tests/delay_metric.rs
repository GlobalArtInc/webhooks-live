mod common;
mod served;

use served::{Configured, endpoint_configured, harness_with_slots};

#[tokio::test]
async fn skipped_delays_are_counted() {
    whl_telemetry::metrics::install().expect("the recorder installs");
    let harness = harness_with_slots(0).await;
    let id = endpoint_configured(
        &harness,
        Configured {
            delay_ms: 1000,
            ..Configured::default()
        },
    )
    .await;

    harness.get(&format!("/{id}")).await;
    harness.get(&format!("/{id}")).await;

    let rendered = whl_telemetry::metrics::render();
    let line = rendered
        .lines()
        .find(|line| line.starts_with("webhooks_live_delay_skipped_total"))
        .expect("the counter is exported");
    assert_eq!(line, "webhooks_live_delay_skipped_total 2");
}
