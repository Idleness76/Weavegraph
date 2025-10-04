use crate::channels::errors::ErrorEvent;
use crate::event_bus::Event;

pub const CONTEXT_COLOR: &str = "\x1b[32m"; // green
pub const LINE_COLOR: &str = "\x1b[35m"; // magenta / dark pink
pub const RESET_COLOR: &str = "\x1b[0m";

/// Rendered output for a telemetry item that can be consumed by sinks.
#[derive(Clone, Debug, Default)]
pub struct EventRender {
    pub context: Option<String>,
    pub lines: Vec<String>,
}

impl EventRender {
    pub fn join_lines(&self) -> String {
        self.lines.join("")
    }
}

pub trait TelemetryFormatter: Send + Sync {
    fn render_event(&self, event: &Event) -> EventRender;
    fn render_errors(&self, errors: &[ErrorEvent]) -> Vec<EventRender>;
}

#[derive(Default)]
pub struct PlainFormatter;

impl TelemetryFormatter for PlainFormatter {
    fn render_event(&self, event: &Event) -> EventRender {
        let line = format!("{LINE_COLOR}{}{RESET_COLOR}\n", event);
        EventRender {
            context: event.scope_label().map(|s| s.to_string()),
            lines: vec![line],
        }
    }

    fn render_errors(&self, errors: &[ErrorEvent]) -> Vec<EventRender> {
        errors
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut lines = Vec::new();
                let scope_str = format!("{CONTEXT_COLOR}{:?}{RESET_COLOR}", e.scope);
                lines.push(format!("[{}] {} | {}\n", i, e.when, scope_str));
                lines.push(format!(
                    "{LINE_COLOR}  error: {}{RESET_COLOR}\n",
                    e.error.message
                ));
                if let Some(cause) = &e.error.cause {
                    lines.push(format!(
                        "{LINE_COLOR}  cause: {}{RESET_COLOR}\n",
                        cause.message
                    ));
                }
                if !e.tags.is_empty() {
                    lines.push(format!("{LINE_COLOR}  tags: {:?}{RESET_COLOR}\n", e.tags));
                }
                if !e.context.is_null() {
                    lines.push(format!(
                        "{LINE_COLOR}  context: {}{RESET_COLOR}\n",
                        e.context
                    ));
                }
                EventRender {
                    context: Some(format!("{:?}", e.scope)),
                    lines,
                }
            })
            .collect()
    }
}
