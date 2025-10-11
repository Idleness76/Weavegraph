use weavegraph::runtimes::types::*;

#[test]
fn test_session_id_creation() {
    let id = SessionId::new("test_session");
    assert_eq!(id.as_str(), "test_session");
    assert_eq!(id.to_string(), "test_session");
}

#[test]
fn test_session_id_generation() {
    let id1 = SessionId::generate();
    let id2 = SessionId::generate();
    // Generated IDs should be different
    assert_ne!(id1, id2);
}

#[test]
fn test_step_number_arithmetic() {
    let step = StepNumber::new(5);
    assert_eq!(step.value(), 5);
    assert_eq!(step.next().value(), 6);
    assert!(!step.is_initial());

    let initial = StepNumber::zero();
    assert!(initial.is_initial());
    assert_eq!(initial.value(), 0);
}

#[test]
fn test_step_number_saturation() {
    let max_step = StepNumber::new(u64::MAX);
    let next = max_step.next();
    assert_eq!(next.value(), u64::MAX); // Should saturate, not overflow
}
