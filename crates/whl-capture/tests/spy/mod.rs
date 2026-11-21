#![allow(dead_code)]

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, PoisonError};

use tracing::field::{Field, Visit};
use tracing::span::{Attributes, Id, Record};
use tracing::subscriber::DefaultGuard;
use tracing::{Event, Metadata, Subscriber};

const SQL_TARGET: &str = "sqlx::query";

#[derive(Debug, Clone)]
pub struct Seen {
    pub target: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct Spy {
    seen: Arc<Mutex<Vec<Seen>>>,
}

struct Collect<'a>(&'a mut String);

impl Visit for Collect<'_> {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        let _ = write!(self.0, "{}={value:?} ", field.name());
    }
}

impl Subscriber for Spy {
    fn enabled(&self, _metadata: &Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &Attributes<'_>) -> Id {
        Id::from_u64(1)
    }

    fn record(&self, _span: &Id, _values: &Record<'_>) {}

    fn record_follows_from(&self, _span: &Id, _follows: &Id) {}

    fn event(&self, event: &Event<'_>) {
        let mut text = String::new();
        event.record(&mut Collect(&mut text));
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Seen {
                target: event.metadata().target().to_owned(),
                text,
            });
    }

    fn enter(&self, _span: &Id) {}

    fn exit(&self, _span: &Id) {}
}

impl Spy {
    pub fn install() -> (Self, DefaultGuard) {
        let spy = Self::default();
        let guard = tracing::subscriber::set_default(spy.clone());
        (spy, guard)
    }

    pub fn statements(&self) -> Vec<String> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .filter(|seen| seen.target == SQL_TARGET)
            .map(|seen| seen.text.clone())
            .collect()
    }

    pub fn statement_count(&self) -> usize {
        self.statements().len()
    }

    pub fn everything(&self) -> String {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|seen| format!("{} {}", seen.target, seen.text))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn clear(&self) {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
    }
}
