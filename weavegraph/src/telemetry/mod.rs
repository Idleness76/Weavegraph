use crate::channels::errors::ErrorEvent;
use crate::event_bus::Event;
use std::io::IsTerminal;

pub const CONTEXT_COLOR: &str = "\x1b[32m"; // green
pub const LINE_COLOR: &str = "\x1b[35m"; // magenta / dark pink
pub const RESET_COLOR: &str = "\x1b[0m";

/// Formatter color mode for telemetry output.
///
/// Controls whether ANSI color codes are included in formatted output:
/// - [`FormatterMode::Auto`]: Automatically detects TTY capability via `stderr.is_terminal()`
/// - [`FormatterMode::Colored`]: Always include color codes (for forced color output)
/// - [`FormatterMode::Plain`]: Never include color codes (for logs/files)
///
/// # Examples
/// ```
/// use weavegraph::telemetry::FormatterMode;
///
/// // Auto-detect based on TTY
/// let mode = FormatterMode::auto_detect();
///
/// // Force colored output
/// let mode = FormatterMode::Colored;
///
/// // Force plain output for logging
/// let mode = FormatterMode::Plain;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FormatterMode {
    /// Auto-detect TTY capability (checks `stderr.is_terminal()`)
    #[default]
    Auto,
    /// Always include ANSI color codes
    Colored,
    /// Never include ANSI color codes
    Plain,
}

impl FormatterMode {
    /// Auto-detect formatter mode based on stderr TTY capability.
    ///
    /// Returns `FormatterMode::Colored` if stderr is a terminal, otherwise `FormatterMode::Plain`.
    pub fn auto_detect() -> Self {
        if std::io::stderr().is_terminal() {
            FormatterMode::Colored
        } else {
            FormatterMode::Plain
        }
    }

    /// Returns true if this mode should use colored output.
    ///
    /// For `Auto` mode, performs TTY detection on each call.
    pub fn is_colored(&self) -> bool {
        match self {
            FormatterMode::Auto => std::io::stderr().is_terminal(),
            FormatterMode::Colored => true,
            FormatterMode::Plain => false,
        }
    }
}

// Default is derived; Auto is the default variant.

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

/// Plain text formatter with optional ANSI color codes.
///
/// Color output is controlled by [`FormatterMode`]:
/// - `Auto`: Uses color when stderr is a TTY
/// - `Colored`: Always uses color
/// - `Plain`: Never uses color
///
/// # Examples
/// ```
/// use weavegraph::telemetry::{PlainFormatter, FormatterMode};
///
/// // Auto-detect TTY
/// let formatter = PlainFormatter::new();
///
/// // Force colored output
/// let formatter = PlainFormatter::with_mode(FormatterMode::Colored);
///
/// // Force plain output (no colors)
/// let formatter = PlainFormatter::with_mode(FormatterMode::Plain);
/// ```
pub struct PlainFormatter {
    mode: FormatterMode,
}

impl PlainFormatter {
    /// Create a new formatter with auto-detected color mode.
    pub fn new() -> Self {
        Self {
            mode: FormatterMode::Auto,
        }
    }

    /// Create a new formatter with explicit color mode.
    pub fn with_mode(mode: FormatterMode) -> Self {
        Self { mode }
    }

    /// Get color prefix string based on current mode.
    fn color<'a>(&self, ansi_code: &'a str) -> &'a str {
        if self.mode.is_colored() {
            ansi_code
        } else {
            ""
        }
    }

    /// Get reset color string based on current mode.
    fn reset(&self) -> &str {
        if self.mode.is_colored() {
            RESET_COLOR
        } else {
            ""
        }
    }
}

impl Default for PlainFormatter {
    fn default() -> Self {
        Self::new()
    }
}

fn format_error_chain(
    error: &crate::channels::errors::LadderError,
    indent: usize,
    use_color: bool,
) -> Vec<String> {
    let mut lines = Vec::new();
    if let Some(cause) = &error.cause {
        let indent_str = "  ".repeat(indent);
        if use_color {
            lines.push(format!(
                "{LINE_COLOR}{}cause: {}{RESET_COLOR}\n",
                indent_str, cause.message
            ));
        } else {
            lines.push(format!("{}cause: {}\n", indent_str, cause.message));
        }
        lines.extend(format_error_chain(cause, indent + 1, use_color));
    }
    lines
}

impl TelemetryFormatter for PlainFormatter {
    fn render_event(&self, event: &Event) -> EventRender {
        let line = if self.mode.is_colored() {
            format!("{LINE_COLOR}{}{RESET_COLOR}\n", event)
        } else {
            format!("{}\n", event)
        };
        EventRender {
            context: event.scope_label().map(|s| s.to_string()),
            lines: vec![line],
        }
    }

    fn render_errors(&self, errors: &[ErrorEvent]) -> Vec<EventRender> {
        let use_color = self.mode.is_colored();
        errors
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let mut lines = Vec::new();
                let scope_str = if use_color {
                    format!("{}{:?}{}", self.color(CONTEXT_COLOR), e.scope, self.reset())
                } else {
                    format!("{:?}", e.scope)
                };
                lines.push(format!("[{}] {} | {}\n", i, e.when, scope_str));

                if use_color {
                    lines.push(format!(
                        "{}  error: {}{}\n",
                        self.color(LINE_COLOR),
                        e.error.message,
                        self.reset()
                    ));
                } else {
                    lines.push(format!("  error: {}\n", e.error.message));
                }

                lines.extend(format_error_chain(&e.error, 1, use_color));

                if !e.tags.is_empty() {
                    if use_color {
                        lines.push(format!(
                            "{}  tags: {:?}{}\n",
                            self.color(LINE_COLOR),
                            e.tags,
                            self.reset()
                        ));
                    } else {
                        lines.push(format!("  tags: {:?}\n", e.tags));
                    }
                }

                if !e.context.is_null() {
                    if use_color {
                        lines.push(format!(
                            "{}  context: {}{}\n",
                            self.color(LINE_COLOR),
                            e.context,
                            self.reset()
                        ));
                    } else {
                        lines.push(format!("  context: {}\n", e.context));
                    }
                }

                EventRender {
                    context: Some(format!("{:?}", e.scope)),
                    lines,
                }
            })
            .collect()
    }
}
