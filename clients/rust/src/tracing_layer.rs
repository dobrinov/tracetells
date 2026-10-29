//! A `tracing` layer: events logged at error level are reported, with their
//! message and `error` field, and the request they happened in when
//! [`crate::TraceLayer`] wraps the service.

use std::fmt::Write;

use tracing::field::{Field, Visit};
use tracing::{Event, Level, Subscriber};
use tracing_subscriber::layer::{Context, Layer};

/// Add to your subscriber: `tracing_subscriber::registry().with(tracetells::TracingLayer)`.
#[derive(Debug, Clone, Copy, Default)]
pub struct TracingLayer;

#[derive(Default)]
struct Fields {
    message: String,
    error: String,
}

impl Visit for Fields {
    fn record_debug(&mut self, field: &Field, value: &dyn std::fmt::Debug) {
        match field.name() {
            "message" => {
                let _ = write!(self.message, "{value:?}");
            }
            "error" | "err" | "e" => {
                let _ = write!(self.error, "{value:?}");
            }
            // Other fields often carry ids, emails or amounts: never sent.
            _ => {}
        }
    }
    fn record_str(&mut self, field: &Field, value: &str) {
        match field.name() {
            "message" => self.message.push_str(value),
            "error" | "err" | "e" => self.error.push_str(value),
            _ => {}
        }
    }
    fn record_error(&mut self, field: &Field, value: &(dyn std::error::Error + 'static)) {
        if matches!(field.name(), "error" | "err" | "e") {
            self.error = value.to_string();
        }
    }
}

impl<S: Subscriber> Layer<S> for TracingLayer {
    fn on_event(&self, event: &Event<'_>, _ctx: Context<'_, S>) {
        if *event.metadata().level() != Level::ERROR || event.metadata().target().starts_with("tracetells") {
            return;
        }
        let mut f = Fields::default();
        event.record(&mut f);
        let message = match (f.message.is_empty(), f.error.is_empty()) {
            (false, false) => format!("{}: {}", f.message, f.error),
            (true, false) => f.error,
            _ => f.message,
        };
        crate::capture(event.metadata().target().to_string(), message, crate::current_request().unwrap_or_default());
    }
}
