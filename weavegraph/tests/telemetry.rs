use chrono::Utc;
use serde_json::json;
use weavegraph::channels::errors::{ErrorEvent, LadderError};
use weavegraph::event_bus::Event;
use weavegraph::telemetry::{
    CONTEXT_COLOR, FormatterMode, LINE_COLOR, PlainFormatter, RESET_COLOR, TelemetryFormatter,
};

#[test]
fn render_event_includes_colors_and_context() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Colored);
    let ev = Event::node_message_with_meta("nodeX", 7, "ScopeX", "hello");
    let render = fmt.render_event(&ev);
    // Context should be set to scope label
    assert_eq!(render.context.as_deref(), Some("ScopeX"));
    // Lines should contain colored body and reset code
    let joined = render.join_lines();
    assert!(joined.contains(LINE_COLOR));
    assert!(joined.contains(RESET_COLOR));
    assert!(joined.contains("hello"));
}

#[test]
fn render_errors_formats_scope_lines_and_details() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Colored);
    let now = Utc::now();

    let mut e1 = ErrorEvent::runner(
        "sess",
        3,
        LadderError::msg("boom").with_cause(LadderError::msg("inner")),
    )
    .with_tag("t1")
    .with_context(json!({"k":1}));
    e1.when = now;

    let mut e2 = ErrorEvent::app(LadderError::msg("oops"));
    e2.when = now;

    let renders = fmt.render_errors(&[e1.clone(), e2.clone()]);
    assert_eq!(renders.len(), 2);

    // First render: should include colored scope, error, cause, tags, and context
    let r0 = renders[0].clone();
    let head = r0.lines[0].clone();
    assert!(head.contains(CONTEXT_COLOR));
    assert!(head.contains(RESET_COLOR));
    let body = r0.lines.join("");
    assert!(body.contains("error: boom"));
    assert!(body.contains("cause: inner"));
    assert!(body.contains("tags: [\"t1\"]"));
    assert!(body.contains("context: {\"k\":1}"));
    assert_eq!(
        r0.context.as_deref(),
        Some("Runner { session: \"sess\", step: 3 }")
    );

    // Second render: App scope with minimal fields
    let r1 = renders[1].clone();
    let hdr = r1.lines[0].clone();
    assert!(hdr.contains("App"));
    let body1 = r1.lines.join("");
    assert!(body1.contains("error: oops"));
    // no cause/tags/context lines should appear
    assert!(!body1.contains("cause:"));
    assert!(!body1.contains("tags:"));
    assert!(!body1.contains("context:"));
}

#[test]
fn formatter_mode_colored_includes_ansi_codes() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Colored);
    let ev = Event::node_message_with_meta("test_node", 1, "TestScope", "test message");
    let render = fmt.render_event(&ev);
    let output = render.join_lines();

    // Should contain ANSI color codes
    assert!(
        output.contains(LINE_COLOR),
        "Colored mode should include LINE_COLOR"
    );
    assert!(
        output.contains(RESET_COLOR),
        "Colored mode should include RESET_COLOR"
    );
}

#[test]
fn formatter_mode_plain_excludes_ansi_codes() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Plain);
    let ev = Event::node_message_with_meta("test_node", 1, "TestScope", "test message");
    let render = fmt.render_event(&ev);
    let output = render.join_lines();

    // Should NOT contain ANSI color codes
    assert!(
        !output.contains('\x1b'),
        "Plain mode should not include any ANSI escape codes"
    );
    assert!(
        output.contains("test message"),
        "Plain mode should still include the message"
    );
}

#[test]
fn formatter_mode_colored_errors_include_colors() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Colored);
    let events = vec![
        ErrorEvent::node("parser", 1, LadderError::msg("Parse error"))
            .with_tag("validation")
            .with_context(json!({"line": 42})),
    ];
    let renders = fmt.render_errors(&events);
    let output = renders[0].join_lines();

    // Should contain color codes in error output
    assert!(output.contains(CONTEXT_COLOR), "Should color the scope");
    assert!(
        output.contains(LINE_COLOR),
        "Should color the error details"
    );
    assert!(output.contains(RESET_COLOR), "Should include reset codes");
}

#[test]
fn formatter_mode_plain_errors_exclude_colors() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Plain);
    let events = vec![
        ErrorEvent::node("parser", 1, LadderError::msg("Parse error"))
            .with_tag("validation")
            .with_context(json!({"line": 42})),
    ];
    let renders = fmt.render_errors(&events);
    let output = renders[0].join_lines();

    // Should NOT contain any ANSI codes
    assert!(
        !output.contains('\x1b'),
        "Plain mode should not include ANSI codes"
    );

    // Should still contain all the content
    assert!(
        output.contains("Parse error"),
        "Should include error message"
    );
    assert!(output.contains("validation"), "Should include tags");
    assert!(output.contains("line"), "Should include context");
}

#[test]
fn formatter_mode_plain_nested_errors_exclude_colors() {
    let fmt = PlainFormatter::with_mode(FormatterMode::Plain);
    let nested_error = LadderError::msg("Root error")
        .with_cause(LadderError::msg("First cause").with_cause(LadderError::msg("Second cause")));

    let events = vec![ErrorEvent::scheduler(1, nested_error)];
    let renders = fmt.render_errors(&events);
    let output = renders[0].join_lines();

    // Should NOT contain ANSI codes even in nested causes
    assert!(
        !output.contains('\x1b'),
        "Plain mode should not include ANSI codes in nested errors"
    );

    // Should still contain all error messages
    assert!(output.contains("Root error"), "Should include root error");
    assert!(output.contains("First cause"), "Should include first cause");
    assert!(
        output.contains("Second cause"),
        "Should include second cause"
    );
}

#[test]
fn formatter_mode_auto_default() {
    // FormatterMode::Auto should be the default
    let mode = FormatterMode::default();
    assert_eq!(mode, FormatterMode::Auto);

    // PlainFormatter::new() should use Auto mode
    let fmt = PlainFormatter::new();
    let default_fmt = PlainFormatter::default();

    // Both should produce the same output (testing structural equivalence)
    let ev = Event::node_message_with_meta("test", 1, "scope", "msg");
    let render1 = fmt.render_event(&ev);
    let render2 = default_fmt.render_event(&ev);
    assert_eq!(render1.join_lines(), render2.join_lines());
}
