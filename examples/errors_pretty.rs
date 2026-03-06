use chrono::{TimeZone, Utc};
use serde_json::json;
use weavegraph::channels::errors::{ErrorEvent, LadderError, pretty_print, pretty_print_with_mode};
use weavegraph::telemetry::FormatterMode;

use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Example demonstrating error event formatting with color mode control.
///
/// By default, `pretty_print()` auto-detects TTY capability and enables colors
/// when stderr is a terminal. You can override this behavior using `pretty_print_with_mode()`:
/// - `FormatterMode::Auto`: Auto-detect (default behavior)
/// - `FormatterMode::Colored`: Force colors on
/// - `FormatterMode::Plain`: Force colors off (for logs/files)
fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_ansi(true),
        )
        .with(
            EnvFilter::from_default_env()
                .add_directive("weavegraph=info".parse().unwrap())
                .add_directive("errors_pretty=info".parse().unwrap()),
        )
        .with(ErrorLayer::default())
        .init();
}

fn main() {
    init_tracing();

    // Sample events across scopes using constructors
    let when0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let when1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 1, 0).unwrap();
    let when2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 2, 0).unwrap();

    let events = vec![
        // App-scoped error with builder pattern
        {
            let mut err = ErrorEvent::app(
                LadderError::msg("application init failure")
                    .with_details(json!({"component":"bootstrap"})),
            )
            .with_tag("startup")
            .with_tag("fatal")
            .with_context(json!({"hint":"check configuration"}));
            err.when = when0; // Override timestamp for consistency
            err
        },
        // Node-scoped error with nested causes
        {
            let mut err = ErrorEvent::node(
                "Other:Parser",
                12,
                LadderError::msg("parse error: unexpected token").with_cause(
                    LadderError::msg("line 3, col 15")
                        .with_cause(LadderError::msg("file corrupted")),
                ),
            )
            .with_tag("retryable")
            .with_context(json!({"file":"/tmp/input.json"}));
            err.when = when1;
            err
        },
        // Runner-scoped error
        {
            let mut err = ErrorEvent::runner(
                "sess-42",
                99,
                LadderError::msg("I/O failure")
                    .with_cause(LadderError::msg("connection reset by peer")),
            )
            .with_context(json!({"remote":"10.0.0.2:443"}));
            err.when = when2;
            err
        },
    ];

    // Auto-detect TTY capability (default behavior)
    let out = pretty_print(&events);
    println!(
        "=== Errors pretty showcase (auto-detect colors) ===\n{}",
        out
    );

    // Example: Force plain output (no colors) - useful for log files
    let plain_out = pretty_print_with_mode(&events, FormatterMode::Plain);
    println!("\n=== Plain output (no colors) ===\n{}", plain_out);
}
