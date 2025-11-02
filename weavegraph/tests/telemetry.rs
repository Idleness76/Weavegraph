use chrono::Utc;
use serde_json::json;
use weavegraph::channels::errors::{ErrorEvent, LadderError};
use weavegraph::event_bus::Event;
use weavegraph::telemetry::{
    PlainFormatter, TelemetryFormatter, CONTEXT_COLOR, LINE_COLOR, RESET_COLOR,
};

#[test]
fn render_event_includes_colors_and_context() {
    let fmt = PlainFormatter;
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
    let fmt = PlainFormatter;
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
