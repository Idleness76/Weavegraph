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
 * Comprehensive Serialization Tests
 ********************/

#[test]
fn test_error_event_serialization_all_scopes() {
    // Test serialization of all ErrorScope variants
    let when = Utc.with_ymd_and_hms(2024, 1, 1, 12, 0, 0).unwrap();

    // Node scope
    let mut node_event = ErrorEvent::node("TestNode", 42, LadderError::msg("node error"))
        .with_tag("test")
        .with_context(json!({"node_id": 42}));
    node_event.when = when;

    let node_json = serde_json::to_value(&node_event).unwrap();
    assert_eq!(node_json["scope"]["scope"], "node");
    assert_eq!(node_json["scope"]["kind"], "TestNode");
    assert_eq!(node_json["scope"]["step"], 42);
    assert_eq!(node_json["error"]["message"], "node error");
    assert_eq!(node_json["tags"], json!(["test"]));
    assert_eq!(node_json["context"]["node_id"], 42);

    // Round-trip test
    let node_deserialized: ErrorEvent = serde_json::from_value(node_json).unwrap();
    assert_eq!(node_deserialized.scope, node_event.scope);
    assert_eq!(node_deserialized.error, node_event.error);
    assert_eq!(node_deserialized.tags, node_event.tags);
    assert_eq!(node_deserialized.context, node_event.context);

    // Scheduler scope
    let mut scheduler_event = ErrorEvent::scheduler(10, LadderError::msg("scheduler error"));
    scheduler_event.when = when;

    let scheduler_json = serde_json::to_value(&scheduler_event).unwrap();
    assert_eq!(scheduler_json["scope"]["scope"], "scheduler");
    assert_eq!(scheduler_json["scope"]["step"], 10);

    let scheduler_deserialized: ErrorEvent = serde_json::from_value(scheduler_json).unwrap();
    assert_eq!(scheduler_deserialized.scope, scheduler_event.scope);

    // Runner scope
    let mut runner_event = ErrorEvent::runner("session-123", 5, LadderError::msg("runner error"));
    runner_event.when = when;

    let runner_json = serde_json::to_value(&runner_event).unwrap();
    assert_eq!(runner_json["scope"]["scope"], "runner");
    assert_eq!(runner_json["scope"]["session"], "session-123");
    assert_eq!(runner_json["scope"]["step"], 5);

    let runner_deserialized: ErrorEvent = serde_json::from_value(runner_json).unwrap();
    assert_eq!(runner_deserialized.scope, runner_event.scope);

    // App scope
    let mut app_event = ErrorEvent::app(LadderError::msg("app error"));
    app_event.when = when;

    let app_json = serde_json::to_value(&app_event).unwrap();
    assert_eq!(app_json["scope"]["scope"], "app");

    let app_deserialized: ErrorEvent = serde_json::from_value(app_json).unwrap();
    assert_eq!(app_deserialized.scope, app_event.scope);
}

#[test]
fn test_ladder_error_nested_serialization() {
    // Test serialization of nested LadderError chains
    let simple_error = LadderError::msg("simple error");
    let simple_json = serde_json::to_value(&simple_error).unwrap();
    assert_eq!(simple_json["message"], "simple error");
    assert!(simple_json["cause"].is_null());
    assert!(simple_json["details"].is_null());

    // Error with details
    let error_with_details =
        LadderError::msg("error with details").with_details(json!({"code": 500, "retry": true}));
    let details_json = serde_json::to_value(&error_with_details).unwrap();
    assert_eq!(details_json["message"], "error with details");
    assert_eq!(details_json["details"]["code"], 500);
    assert_eq!(details_json["details"]["retry"], true);

    // Round-trip test
    let details_deserialized: LadderError = serde_json::from_value(details_json).unwrap();
    assert_eq!(details_deserialized, error_with_details);

    // Error with cause
    let error_with_cause =
        LadderError::msg("outer error").with_cause(LadderError::msg("inner error"));
    let cause_json = serde_json::to_value(&error_with_cause).unwrap();
    assert_eq!(cause_json["message"], "outer error");
    assert_eq!(cause_json["cause"]["message"], "inner error");
    assert!(cause_json["cause"]["cause"].is_null());

    // Round-trip test
    let cause_deserialized: LadderError = serde_json::from_value(cause_json).unwrap();
    assert_eq!(cause_deserialized, error_with_cause);

    // Deeply nested error chain
    let deep_error = LadderError::msg("level 1").with_cause(
        LadderError::msg("level 2")
            .with_cause(LadderError::msg("level 3").with_details(json!({"deep": true}))),
    );
    let deep_json = serde_json::to_value(&deep_error).unwrap();
    assert_eq!(deep_json["message"], "level 1");
    assert_eq!(deep_json["cause"]["message"], "level 2");
    assert_eq!(deep_json["cause"]["cause"]["message"], "level 3");
    assert_eq!(deep_json["cause"]["cause"]["details"]["deep"], true);

    // Round-trip test for complex error
    let deep_deserialized: LadderError = serde_json::from_value(deep_json).unwrap();
    assert_eq!(deep_deserialized, deep_error);
}

#[test]
fn test_error_event_full_serialization_roundtrip() {
    // Test complete ErrorEvent with all fields populated
    let original = ErrorEvent::node(
        "ComplexNode",
        99,
        LadderError::msg("complex error")
            .with_cause(LadderError::msg("root cause"))
            .with_details(json!({"severity": "high"})),
    )
    .with_tags(vec!["critical".to_string(), "network".to_string()])
    .with_context(json!({
        "user_id": 12345,
        "request_id": "req-abc-123",
        "endpoint": "/api/process",
        "metadata": {
            "version": "1.2.3",
            "environment": "production"
        }
    }));

    // Serialize to JSON
    let json_str = serde_json::to_string(&original).unwrap();
    let json_value: serde_json::Value = serde_json::from_str(&json_str).unwrap();

    // Verify structure
    assert_eq!(json_value["scope"]["scope"], "node");
    assert_eq!(json_value["scope"]["kind"], "ComplexNode");
    assert_eq!(json_value["scope"]["step"], 99);
    assert_eq!(json_value["error"]["message"], "complex error");
    assert_eq!(json_value["error"]["cause"]["message"], "root cause");
    assert_eq!(json_value["error"]["details"]["severity"], "high");
    assert_eq!(json_value["tags"], json!(["critical", "network"]));
    assert_eq!(json_value["context"]["user_id"], 12345);
    assert_eq!(json_value["context"]["request_id"], "req-abc-123");
    assert_eq!(json_value["context"]["endpoint"], "/api/process");
    assert_eq!(json_value["context"]["metadata"]["version"], "1.2.3");
    assert_eq!(
        json_value["context"]["metadata"]["environment"],
        "production"
    );

    // Deserialize back
    let deserialized: ErrorEvent = serde_json::from_str(&json_str).unwrap();
    assert_eq!(deserialized.scope, original.scope);
    assert_eq!(deserialized.error, original.error);
    assert_eq!(deserialized.tags, original.tags);
    assert_eq!(deserialized.context, original.context);
}

#[test]
fn test_error_event_schema_stability() {
    // This test serves as a regression check - if the schema changes unexpectedly,
    // this test will fail and alert us to review the change

    let event = ErrorEvent::node("SchemaTest", 1, LadderError::msg("test"))
        .with_tag("regression")
        .with_context(json!({"test": true}));

    let json = serde_json::to_value(&event).unwrap();

    // Check that all expected top-level fields exist
    assert!(json.get("when").is_some(), "Missing 'when' field");
    assert!(json.get("scope").is_some(), "Missing 'scope' field");
    assert!(json.get("error").is_some(), "Missing 'error' field");
    assert!(json.get("tags").is_some(), "Missing 'tags' field");
    assert!(json.get("context").is_some(), "Missing 'context' field");

    // Check scope structure
    let scope = &json["scope"];
    assert!(
        scope.get("scope").is_some(),
        "Missing 'scope.scope' discriminator"
    );

    // Check error structure
    let error = &json["error"];
    assert!(
        error.get("message").is_some(),
        "Missing 'error.message' field"
    );
    // Note: cause and details may be omitted if null/empty due to skip_serializing_if

    // Ensure tags is an array
    assert!(json["tags"].is_array(), "tags should be an array");

    // Ensure context can be any JSON value
    assert!(
        json["context"].is_object() || json["context"].is_null(),
        "context should be object or null"
    );
}

#[test]
fn test_error_scope_variants_complete_coverage() {
    // Ensure all scope variants serialize and deserialize correctly
    let test_cases = vec![
        (
            ErrorScope::Node {
                kind: "MyNode".to_string(),
                step: 1,
            },
            json!({"scope": "node", "kind": "MyNode", "step": 1}),
        ),
        (
            ErrorScope::Scheduler { step: 5 },
            json!({"scope": "scheduler", "step": 5}),
        ),
        (
            ErrorScope::Runner {
                session: "sess-x".to_string(),
                step: 10,
            },
            json!({"scope": "runner", "session": "sess-x", "step": 10}),
        ),
        (ErrorScope::App, json!({"scope": "app"})),
    ];

    for (scope, expected_json) in test_cases {
        // Serialize
        let serialized = serde_json::to_value(&scope).unwrap();
        assert_eq!(
            serialized, expected_json,
            "Serialization mismatch for {:?}",
            scope
        );

        // Deserialize
        let deserialized: ErrorScope = serde_json::from_value(serialized).unwrap();
        assert_eq!(deserialized, scope, "Round-trip failed for {:?}", scope);
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

#[test]
fn pretty_print_with_mode_colored_includes_ansi_codes() {
    use weavegraph::telemetry::FormatterMode;

    let event =
        ErrorEvent::node("parser", 1, LadderError::msg("Parse failed")).with_tag("validation");
    let events = vec![event];

    let output = pretty_print_with_mode(&events, FormatterMode::Colored);

    // Should contain ANSI escape codes
    assert!(
        output.contains('\x1b'),
        "Colored mode should include ANSI escape codes"
    );
    assert!(
        output.contains("Parse failed"),
        "Should include error message"
    );
    assert!(output.contains("validation"), "Should include tags");
}

#[test]
fn pretty_print_with_mode_plain_excludes_ansi_codes() {
    use weavegraph::telemetry::FormatterMode;

    let nested_error =
        LadderError::msg("Top level error").with_cause(LadderError::msg("Nested cause"));
    let event = ErrorEvent::scheduler(5, nested_error)
        .with_tag("critical")
        .with_context(json!({"attempt": 3}));
    let events = vec![event];

    let output = pretty_print_with_mode(&events, FormatterMode::Plain);

    // Should NOT contain any ANSI escape codes
    assert!(
        !output.contains('\x1b'),
        "Plain mode should not include ANSI escape codes"
    );

    // Should still contain all content
    assert!(
        output.contains("Top level error"),
        "Should include root error"
    );
    assert!(
        output.contains("Nested cause"),
        "Should include nested cause"
    );
    assert!(output.contains("critical"), "Should include tags");
    assert!(output.contains("attempt"), "Should include context");
}

#[test]
fn pretty_print_uses_auto_mode_by_default() {
    use weavegraph::telemetry::FormatterMode;

    let event = ErrorEvent::app(LadderError::msg("Test error"));
    let events = vec![event.clone()];

    let auto_output = pretty_print(&events);
    let explicit_auto_output = pretty_print_with_mode(&events, FormatterMode::Auto);

    // Both should produce identical output
    assert_eq!(
        auto_output, explicit_auto_output,
        "pretty_print should be equivalent to pretty_print_with_mode(Auto)"
    );
}
