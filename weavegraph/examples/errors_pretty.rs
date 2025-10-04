use chrono::{TimeZone, Utc};
use serde_json::json;
use weavegraph::channels::errors::{pretty_print, ErrorEvent, ErrorScope, LadderError};

fn main() {
    // Sample events across scopes with a nested cause chain and context/tags
    let when0 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let when1 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 1, 0).unwrap();
    let when2 = Utc.with_ymd_and_hms(2024, 1, 1, 0, 2, 0).unwrap();

    let events = vec![
        ErrorEvent {
            when: when0,
            scope: ErrorScope::App,
            error: LadderError::msg("application init failure")
                .with_details(json!({"component":"bootstrap"})),
            tags: vec!["startup".into(), "fatal".into()],
            context: json!({"hint":"check configuration"}),
        },
        ErrorEvent {
            when: when1,
            scope: ErrorScope::Node {
                kind: "Other:Parser".into(),
                step: 12,
            },
            error: LadderError::msg("parse error: unexpected token")
                .with_cause(LadderError::msg("line 3, col 15")),
            tags: vec!["retryable".into()],
            context: json!({"file":"/tmp/input.json"}),
        },
        ErrorEvent {
            when: when2,
            scope: ErrorScope::Runner {
                session: "sess-42".into(),
                step: 99,
            },
            error: LadderError::msg("I/O failure")
                .with_cause(LadderError::msg("connection reset by peer")),
            tags: vec![],
            context: json!({"remote":"10.0.0.2:443"}),
        },
    ];

    let out = pretty_print(&events);
    println!("=== Errors pretty showcase ===\n{}", out);
}
