use chrono::{TimeZone, Utc};
use serde_json::json;

use weavegraph::channels::errors::*;
use weavegraph::channels::{Channel, ErrorsChannel};
use weavegraph::types::ChannelType;

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
    let node = ErrorScope::Node {
        kind: "Custom:Parser".into(),
        step: 42,
    };
    let ser_node = serde_json::to_value(&node).unwrap();
    assert_eq!(ser_node["scope"], "node");
    assert_eq!(ser_node["kind"], "Custom:Parser");
    assert_eq!(ser_node["step"], 42);

    let sch = ErrorScope::Scheduler { step: 10 };
    let ser_sch = serde_json::to_value(&sch).unwrap();
    assert_eq!(ser_sch["scope"], "scheduler");

    let run = ErrorScope::Runner {
        session: "abc".into(),
        step: 7,
    };
    let ser_run = serde_json::to_value(&run).unwrap();
    assert_eq!(ser_run["scope"], "runner");

    let app = ErrorScope::App;
    let ser_app = serde_json::to_value(&app).unwrap();
    assert_eq!(ser_app["scope"], "app");

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
 * ErrorEvent Constructor Tests
 ********************/

#[test]
fn error_event_node_constructor() {
    let err = ErrorEvent::node("Parser", 42, LadderError::msg("parse failed"));

    assert!(matches!(err.scope, ErrorScope::Node { .. }));
    if let ErrorScope::Node { kind, step } = err.scope {
        assert_eq!(kind, "Parser");
        assert_eq!(step, 42);
    }
    assert_eq!(err.error.message, "parse failed");
    assert!(err.tags.is_empty());
    assert!(err.context.is_null());
}

#[test]
fn error_event_scheduler_constructor() {
    let err = ErrorEvent::scheduler(10, LadderError::msg("scheduling conflict"));

    assert!(matches!(err.scope, ErrorScope::Scheduler { .. }));
    if let ErrorScope::Scheduler { step } = err.scope {
        assert_eq!(step, 10);
    }
    assert_eq!(err.error.message, "scheduling conflict");
    assert!(err.tags.is_empty());
    assert!(err.context.is_null());
}

#[test]
fn error_event_runner_constructor() {
    let err = ErrorEvent::runner("session-abc", 99, LadderError::msg("runtime error"));

    assert!(matches!(err.scope, ErrorScope::Runner { .. }));
    if let ErrorScope::Runner { session, step } = err.scope {
        assert_eq!(session, "session-abc");
        assert_eq!(step, 99);
    }
    assert_eq!(err.error.message, "runtime error");
    assert!(err.tags.is_empty());
    assert!(err.context.is_null());
}

#[test]
fn error_event_app_constructor() {
    let err = ErrorEvent::app(LadderError::msg("startup failed"));

    assert!(matches!(err.scope, ErrorScope::App));
    assert_eq!(err.error.message, "startup failed");
    assert!(err.tags.is_empty());
    assert!(err.context.is_null());
}

#[test]
fn error_event_with_tag_builder() {
    let err = ErrorEvent::node("Validator", 1, LadderError::msg("invalid")).with_tag("validation");

    assert_eq!(err.tags, vec!["validation"]);
}

#[test]
fn error_event_with_multiple_tags_chained() {
    let err = ErrorEvent::scheduler(5, LadderError::msg("error"))
        .with_tag("critical")
        .with_tag("retry");

    assert_eq!(err.tags, vec!["critical", "retry"]);
}

#[test]
fn error_event_with_tags_builder() {
    let err = ErrorEvent::runner("sess-1", 3, LadderError::msg("failed"))
        .with_tags(vec!["urgent".to_string(), "logged".to_string()]);

    assert_eq!(err.tags, vec!["urgent", "logged"]);
}

#[test]
fn error_event_with_context_builder() {
    let err = ErrorEvent::app(LadderError::msg("config error"))
        .with_context(json!({"config_file": "/etc/app.conf", "line": 42}));

    assert_eq!(err.context["config_file"], "/etc/app.conf");
    assert_eq!(err.context["line"], 42);
}

#[test]
fn error_event_full_builder_chain() {
    let err = ErrorEvent::node(
        "Analyzer",
        7,
        LadderError::msg("analysis failed")
            .with_cause(LadderError::msg("missing data"))
            .with_details(json!({"field": "input"})),
    )
    .with_tag("retryable")
    .with_tag("logged")
    .with_context(json!({"attempt": 3, "max_attempts": 5}));

    // Verify scope
    if let ErrorScope::Node { kind, step } = err.scope {
        assert_eq!(kind, "Analyzer");
        assert_eq!(step, 7);
    } else {
        panic!("Expected Node scope");
    }

    // Verify error chain
    assert_eq!(err.error.message, "analysis failed");
    assert!(err.error.cause.is_some());
    assert_eq!(err.error.cause.as_ref().unwrap().message, "missing data");
    assert_eq!(err.error.details["field"], "input");

    // Verify tags
    assert_eq!(err.tags, vec!["retryable", "logged"]);

    // Verify context
    assert_eq!(err.context["attempt"], 3);
    assert_eq!(err.context["max_attempts"], 5);
}

#[test]
fn error_event_constructors_serialize_correctly() {
    // Test that constructed events serialize the same as manual construction
    let manual = ErrorEvent {
        when: Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap(),
        scope: ErrorScope::Node {
            kind: "Test".to_string(),
            step: 1,
        },
        error: LadderError::msg("test"),
        tags: vec!["tag1".to_string()],
        context: json!({"key": "value"}),
    };

    let constructed = ErrorEvent::node("Test", 1, LadderError::msg("test"))
        .with_tag("tag1")
        .with_context(json!({"key": "value"}));

    // Serialize both (ignore timestamp difference)
    let manual_json = serde_json::to_value(&manual).unwrap();
    let constructed_json = serde_json::to_value(&constructed).unwrap();

    // Compare everything except timestamp
    assert_eq!(manual_json["scope"], constructed_json["scope"]);
    assert_eq!(manual_json["error"], constructed_json["error"]);
    assert_eq!(manual_json["tags"], constructed_json["tags"]);
    assert_eq!(manual_json["context"], constructed_json["context"]);
}

#[test]
fn error_event_string_into_conversions() {
    // Test that Into<String> works for both &str and String
    let from_str = ErrorEvent::node("literal", 1, LadderError::msg("test"));
    let from_string = ErrorEvent::node(String::from("owned"), 1, LadderError::msg("test"));

    if let ErrorScope::Node { kind, .. } = from_str.scope {
        assert_eq!(kind, "literal");
    }

    if let ErrorScope::Node { kind, .. } = from_string.scope {
        assert_eq!(kind, "owned");
    }

    let runner_str = ErrorEvent::runner("session", 1, LadderError::msg("test"));
    let runner_string = ErrorEvent::runner(String::from("session_id"), 1, LadderError::msg("test"));

    if let ErrorScope::Runner { session, .. } = runner_str.scope {
        assert_eq!(session, "session");
    }

    if let ErrorScope::Runner { session, .. } = runner_string.scope {
        assert_eq!(session, "session_id");
    }
}

/********************
 * pretty_print tests
 ********************/

#[test]
fn pretty_print_renders_usefully() {
    let when = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
    let mut event = ErrorEvent::runner(
        "sess-1",
        3,
        LadderError::msg("failed").with_cause(LadderError::msg("io")),
    )
    .with_tag("urgent")
    .with_context(json!({"path":"/tmp/x"}));

    // Override timestamp for test consistency
    event.when = when;

    let events = vec![event];

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

    let when = Utc::now();

    // Add first error using scheduler constructor
    let err1 = ErrorEvent::scheduler(1, LadderError::msg("first"));
    let mut err1_with_time = err1;
    err1_with_time.when = when;
    ch.get_mut().push(err1_with_time);

    // Add second error using node constructor with builder
    let err2 = ErrorEvent::node("Start", 2, LadderError::msg("second"))
        .with_tag("retryable")
        .with_context(json!({"try":2}));
    let mut err2_with_time = err2;
    err2_with_time.when = when;
    ch.get_mut().push(err2_with_time);

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
    let mut e = ErrorEvent::app(LadderError::msg("boom"));
    e.when = when;

    let ch = ErrorsChannel::new(vec![e.clone()], 7);
    assert_eq!(ch.version(), 7);
    assert_eq!(ch.snapshot(), vec![e]);
}

#[test]
fn optional_cli_pretty_demo() {
    let when = Utc.with_ymd_and_hms(2024, 2, 2, 2, 2, 2).unwrap();
    let mut event = ErrorEvent::app(LadderError::msg("display"))
        .with_tag("cli")
        .with_context(json!({}));
    event.when = when;

    let events = vec![event];

    let out = pretty_print(&events);
    println!("\n=== Errors pretty showcase ===\n{}", out);
    assert!(out.contains("display"));
}
