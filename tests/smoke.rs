//! Smoke tests that run examples to ensure they work end-to-end.
//!
//! These tests are disabled by default to avoid slowing down the regular test suite.
//! Enable them by setting the WEAVEGRAPH_SMOKE_TESTS environment variable:
//!
//!     WEAVEGRAPH_SMOKE_TESTS=1 cargo test smoke
//!
//! Or run all tests including smoke tests:
//!
//!     WEAVEGRAPH_SMOKE_TESTS=1 cargo test
//!
//! TODO: Integrate smoke tests into CI pipeline
//! - Add WEAVEGRAPH_SMOKE_TESTS=1 to CI environment variables
//! - Consider running smoke tests on a schedule or for releases
//! - Evaluate if additional examples should be included in smoke tests

use std::process::Command;

/// Helper to run an example and verify it succeeds with output
fn run_example(example_name: &str) {
    let result = Command::new("cargo")
        .args(["run", "--example", example_name])
        .output()
        .unwrap_or_else(|_| panic!("Failed to run example: {}", example_name));

    assert!(
        result.status.success(),
        "Example '{}' failed with exit code {:?}\n\nSTDOUT:\n{}\n\nSTDERR:\n{}",
        example_name,
        result.status.code(),
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );

    // Verify there's some output (examples should produce logging/tracing output)
    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let combined_output = format!("{}{}", stdout, stderr);

    assert!(
        !combined_output.trim().is_empty(),
        "Example '{}' produced no output",
        example_name
    );
}

#[test]
fn smoke_test_basic_nodes() {
    if std::env::var("WEAVEGRAPH_SMOKE_TESTS").is_err() {
        eprintln!(
            "Skipping smoke test smoke_test_basic_nodes (set WEAVEGRAPH_SMOKE_TESTS=1 to enable)"
        );
        return;
    }

    run_example("basic_nodes");
}

#[test]
fn smoke_test_demo1() {
    if std::env::var("WEAVEGRAPH_SMOKE_TESTS").is_err() {
        eprintln!("Skipping smoke test smoke_test_demo1 (set WEAVEGRAPH_SMOKE_TESTS=1 to enable)");
        return;
    }

    run_example("demo1");
}

#[test]
fn smoke_test_errors_pretty() {
    if std::env::var("WEAVEGRAPH_SMOKE_TESTS").is_err() {
        eprintln!(
            "Skipping smoke test smoke_test_errors_pretty (set WEAVEGRAPH_SMOKE_TESTS=1 to enable)"
        );
        return;
    }

    run_example("errors_pretty");
}
