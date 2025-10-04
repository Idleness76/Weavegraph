use super::errors::*;
use super::ErrorsChannel;
use crate::channels::Channel;
use crate::types::ChannelType;
use chrono::{TimeZone, Utc};
use serde_json::json;

/********************
 * LadderError tests
 ********************/

#[test]
fn ladder_error_msg_and_chain() {
    let base = LadderError::msg("root cause").with_details(json!({"k":"v"}));
    let wrapped = LadderError::msg("top").with_cause(base.clone());

    assert_eq!(base.message, "root cause");
    assert_eq!(wrapped.message, "top");
    assert!(wrapped.cause.is_some());
    assert_eq!(wrapped.cause.as_ref().unwrap().message, base.message);
    assert_eq!(base.details, json!({"k":"v"}));
}

#[test]
fn ladder_error_serde_roundtrip() {
    let err = LadderError::msg("boom")
        .with_details(json!({"code": 500}))
        .with_cause(LadderError::msg("inner"));

    let ser = serde_json::to_string(&err).expect("serialize");
    let de: LadderError = serde_json::from_str(&ser).expect("deserialize");
    assert_eq!(de, err);
}

/********************
 * ErrorScope tests
 ********************/

#[test]
fn error_scope_enum_variants_serde() {
    // Node scope with kind encoded externally as string
    let node = ErrorScope::Node {
        kind: "Other:Parser".into(),
        step: 42,
    };
    let ser_node = serde_json::to_value(&node).unwrap();
    assert_eq!(ser_node["scope"], "node");
    assert_eq!(ser_node["kind"], "Other:Parser");
    assert_eq!(ser_node["step"], 42);

    // Scheduler
    let sch = ErrorScope::Scheduler { step: 10 };
    let ser_sch = serde_json::to_value(&sch).unwrap();
    assert_eq!(ser_sch["scope"], "scheduler");

    // Runner
    let run = ErrorScope::Runner {
        session: "abc".into(),
        step: 7,
    };
    let ser_run = serde_json::to_value(&run).unwrap();
    assert_eq!(ser_run["scope"], "runner");

    // App
    let app = ErrorScope::App;
    let ser_app = serde_json::to_value(&app).unwrap();
    assert_eq!(ser_app["scope"], "app");

    // Roundtrip
    assert_eq!(
        serde_json::from_value::<ErrorScope>(ser_node).unwrap(),
        node
    );
    assert_eq!(serde_json::from_value::<ErrorScope>(ser_sch).unwrap(), sch);
    assert_eq!(serde_json::from_value::<ErrorScope>(ser_run).unwrap(), run);
    assert_eq!(serde_json::from_value::<ErrorScope>(ser_app).unwrap(), app);
}

/********************
 * ErrorEvent tests
 ********************/

#[test]
fn error_event_defaults_and_roundtrip() {
    let when = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
    let ev = ErrorEvent {
        when,
        scope: ErrorScope::App,
        error: LadderError::msg("oops"),
        tags: vec!["t1".into(), "t2".into()],
        context: json!({"info": true}),
    };

    let ser = serde_json::to_string(&ev).unwrap();
    let de: ErrorEvent = serde_json::from_str(&ser).unwrap();
    assert_eq!(de, ev);
}

#[test]
fn error_event_defaults_are_empty_when_missing() {
    let when = Utc.with_ymd_and_hms(2024, 5, 6, 7, 8, 9).unwrap();
    // Internally tagged enums nested inside a struct are encoded as an object with the tag inside
    let v = json!({
        "when": when,
        "scope": {"scope": "app"},
        "error": {"message":"x"}
    });
    let de: ErrorEvent = serde_json::from_value(v).unwrap();
    assert!(de.tags.is_empty());
    assert!(de.context.is_null());
}

/********************
 * pretty_print tests
 ********************/

#[test]
fn pretty_print_renders_usefully() {
    let when = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let events = vec![ErrorEvent {
        when,
        scope: ErrorScope::Runner {
            session: "sess-1".into(),
            step: 3,
        },
        error: LadderError::msg("failed").with_cause(LadderError::msg("io")),
        tags: vec!["urgent".into()],
        context: json!({"path":"/tmp/x"}),
    }];

    let out = pretty_print(&events);
    assert!(out.contains("failed"));
    assert!(out.contains("cause: io"));
    assert!(out.contains("Runner"));
    assert!(out.contains("sess-1"));
    assert!(out.contains("/tmp/x"));
}

/********************
 * ErrorsChannel tests
 ********************/

#[test]
fn errors_channel_basics() {
    let mut ch = ErrorsChannel::default();
    assert_eq!(ch.get_channel_type(), ChannelType::Error);
    assert!(ch.persistent());
    assert_eq!(ch.version(), 1);
    assert_eq!(ch.len(), 0);
    assert!(ch.is_empty());

    // insert a couple of events
    let when = Utc::now();
    ch.get_mut().push(ErrorEvent {
        when,
        scope: ErrorScope::Scheduler { step: 1 },
        error: LadderError::msg("first"),
        tags: vec![],
        context: serde_json::Value::Null,
    });

    ch.get_mut().push(ErrorEvent {
        when,
        scope: ErrorScope::Node {
            kind: "Start".into(),
            step: 2,
        },
        error: LadderError::msg("second"),
        tags: vec!["retryable".into()],
        context: json!({"try":2}),
    });

    assert_eq!(ch.len(), 2);
    assert!(!ch.is_empty());

    let snap = ch.snapshot();
    assert_eq!(snap.len(), 2);
    assert_eq!(snap[0].error.message, "first");
    assert_eq!(snap[1].tags, vec!["retryable"]);

    ch.set_version(5);
    assert_eq!(ch.version(), 5);
}

#[test]
fn errors_channel_new_constructor() {
    let when = Utc::now();
    let e = ErrorEvent {
        when,
        scope: ErrorScope::App,
        error: LadderError::msg("boom"),
        tags: vec![],
        context: serde_json::Value::Null,
    };
    let ch = ErrorsChannel::new(vec![e.clone()], 7);
    assert_eq!(ch.version(), 7);
    assert_eq!(ch.snapshot(), vec![e]);
}

/********************
 * Optional CLI pretty demo (non-fatal)
 ********************/

#[test]
fn optional_cli_pretty_demo() {
    // This test just ensures pretty_print output can be produced without panics
    let when = Utc.with_ymd_and_hms(2024, 2, 2, 2, 2, 2).unwrap();
    let events = vec![ErrorEvent {
        when,
        scope: ErrorScope::App,
        error: LadderError::msg("display"),
        tags: vec!["cli".into()],
        context: json!({}),
    }];

    let out = pretty_print(&events);
    // print to stdout for showcase effect when running `cargo test -q`
    println!("\n=== Errors pretty showcase ===\n{}", out);
    assert!(out.contains("display"));
}
