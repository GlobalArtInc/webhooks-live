use metrics::{counter, gauge};
use whl_telemetry::series::{
    LISTENER_RECONNECTS_TOTAL, SSE_CONNECTIONS, SSE_EVENTS_TOTAL, SSE_RESYNCS_TOTAL,
};

use crate::event::ResyncReason;

#[derive(Debug)]
pub(crate) struct ConnectionGauge;

impl ConnectionGauge {
    pub(crate) fn open() -> Self {
        gauge!(SSE_CONNECTIONS).increment(1.0);
        Self
    }
}

impl Drop for ConnectionGauge {
    fn drop(&mut self) {
        gauge!(SSE_CONNECTIONS).decrement(1.0);
    }
}

pub(crate) fn event_sent(event: &'static str) {
    counter!(SSE_EVENTS_TOTAL, "event" => event).increment(1);
}

pub(crate) fn resync_sent(reason: ResyncReason) {
    counter!(SSE_RESYNCS_TOTAL, "reason" => reason.as_str()).increment(1);
}

pub(crate) fn listener_reconnected() {
    counter!(LISTENER_RECONNECTS_TOTAL).increment(1);
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex, PoisonError};

    use metrics::{
        Counter, CounterFn, Gauge, GaugeFn, Histogram, Key, KeyName, Metadata, Recorder,
        SharedString, Unit,
    };

    use super::*;

    type Totals = Arc<Mutex<HashMap<String, f64>>>;

    struct Cell {
        key: String,
        totals: Totals,
    }

    impl Cell {
        fn add(&self, delta: f64) {
            let mut totals = self.totals.lock().unwrap_or_else(PoisonError::into_inner);
            *totals.entry(self.key.clone()).or_insert(0.0) += delta;
        }
    }

    impl CounterFn for Cell {
        fn increment(&self, value: u64) {
            self.add(value as f64);
        }

        fn absolute(&self, value: u64) {
            self.add(value as f64);
        }
    }

    impl GaugeFn for Cell {
        fn increment(&self, value: f64) {
            self.add(value);
        }

        fn decrement(&self, value: f64) {
            self.add(-value);
        }

        fn set(&self, value: f64) {
            self.add(value);
        }
    }

    struct Capture {
        totals: Totals,
    }

    impl Capture {
        fn cell(&self, key: &Key) -> Arc<Cell> {
            let labels: Vec<String> = key
                .labels()
                .map(|label| format!("{}={}", label.key(), label.value()))
                .collect();
            Arc::new(Cell {
                key: format!("{}{{{}}}", key.name(), labels.join(",")),
                totals: Arc::clone(&self.totals),
            })
        }
    }

    impl Recorder for Capture {
        fn describe_counter(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_gauge(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}
        fn describe_histogram(&self, _: KeyName, _: Option<Unit>, _: SharedString) {}

        fn register_counter(&self, key: &Key, _: &Metadata<'_>) -> Counter {
            Counter::from_arc(self.cell(key))
        }

        fn register_gauge(&self, key: &Key, _: &Metadata<'_>) -> Gauge {
            Gauge::from_arc(self.cell(key))
        }

        fn register_histogram(&self, _: &Key, _: &Metadata<'_>) -> Histogram {
            Histogram::noop()
        }
    }

    #[test]
    fn emits_the_documented_series_with_their_labels() {
        let totals = Totals::default();
        let recorder = Capture {
            totals: Arc::clone(&totals),
        };
        let read = |key: &str| {
            totals
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .get(key)
                .copied()
        };

        metrics::with_local_recorder(&recorder, || {
            let first = ConnectionGauge::open();
            let second = ConnectionGauge::open();
            assert_eq!(read("webhooks_live_sse_connections{}"), Some(2.0));
            drop(second);
            assert_eq!(read("webhooks_live_sse_connections{}"), Some(1.0));
            event_sent("capture");
            event_sent("capture");
            resync_sent(ResyncReason::Lagged);
            listener_reconnected();
            drop(first);
        });

        assert_eq!(read("webhooks_live_sse_connections{}"), Some(0.0));
        assert_eq!(
            read("webhooks_live_sse_events_total{event=capture}"),
            Some(2.0)
        );
        assert_eq!(
            read("webhooks_live_sse_resyncs_total{reason=lagged}"),
            Some(1.0)
        );
        assert_eq!(read("webhooks_live_listener_reconnects_total{}"), Some(1.0));
    }
}
