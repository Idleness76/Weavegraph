//! Demo 1: Basic Graph Building and Execution
//!
//! This demonstration showcases the fundamental graph building and execution patterns
//! in the Weavegraph framework. It covers basic workflow construction, state management,
//! barrier operations, and error handling scenarios.
//!
//! What You'll Learn:
//! 1. Modern Message Construction: Typed roles with `Message::with_role()`
//! 2. State Management: Working with versioned state and snapshots
//! 3. Graph Building: Creating workflows with nodes and edges
//! 4. Barrier Operations: Manual state updates and version management
//! 5. Error Handling: Validation and expected failure scenarios
//!
//! Running This Demo:
//! ```bash
//! cargo run --example demo1
//! ```

use async_trait::async_trait;
use miette::Result;
use rustc_hash::FxHashMap;
use serde_json::json;
use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use weavegraph::channels::Channel;
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

/// Simple demonstration node that adds an assistant message.
///
/// This node demonstrates the modern patterns for:
/// - Using convenience constructors for messages
/// - Returning well-formed `NodePartial` results
/// - Basic async node implementation
#[derive(Clone)]
struct SimpleNode {
    name: String,
}

impl SimpleNode {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

#[async_trait]
impl Node for SimpleNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        // Emit a progress event (modern pattern)
        ctx.emit(
            "node_execution",
            format!("Node {} starting execution", self.name),
        )?;

        // Get the last message to process (if any)
        let input_msg = snapshot.messages.last();
        let response = match input_msg {
            Some(msg) => format!("Node {} processed: {}", self.name, msg.content),
            None => format!("Node {} initialized with no input", self.name),
        };

        // ✅ MODERN: Use typed roles for message construction
        let output_message = Message::with_role(Role::Assistant, &response);

        ctx.emit(
            "node_completion",
            format!("Node {} completed successfully", self.name),
        )?;

        Ok(NodePartial::new().with_messages(vec![output_message]))
    }
}

/// Demonstration showcasing basic graph building and execution patterns.
///
/// This demo illustrates:
/// 1. Modern message and state construction patterns
/// 2. Simple graph building with the GraphBuilder API
/// 3. Full workflow execution using the `invoke` method
/// 4. State snapshots and mutations
/// 5. Manual barrier operations for advanced use cases
/// 6. Error handling and validation scenarios
///
/// # Key Modern Patterns Demonstrated
///
/// - **Message Construction**: `Message::with_role(Role::User, ...)` for typed roles
/// - **State Building**: `VersionedState::builder()` for complex initialization
/// - **Error Handling**: Proper Result types and error propagation
/// - **Event Emission**: Using `NodeContext::emit()` for observability
///
/// # Expected Output
///
/// The demo will show:
/// - Graph compilation and execution
/// - State snapshots before and after mutations
/// - Barrier operation results with channel updates
/// - Expected error cases for validation
fn init_tracing() {
    let fmt_layer = fmt::layer()
        .with_target(false)
        .with_file(false)
        .with_line_number(false)
        // Log when spans are created/closed so we see instrumented async boundaries
        .with_span_events(FmtSpan::NEW | FmtSpan::CLOSE);

    let filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new("error,weavegraph=error"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt_layer)
        .with(ErrorLayer::default())
        .init();
}

fn init_miette() {
    // Pretty panic reports
    miette::set_panic_hook();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    init_miette();
    demo().await
}

async fn demo() -> Result<()> {
    info!("\n╔══════════════════════════════════════════════════════════╗");
    info!("║                        Demo 1                           ║");
    info!("║              Basic Graph Building & Execution           ║");
    info!("╚══════════════════════════════════════════════════════════╝\n");

    // ✅ STEP 1: Modern State Construction
    info!("📊 Step 1: Creating initial state with modern patterns");

    // Using the builder pattern for rich initial state
    let init = VersionedState::builder()
        .with_user_message("Hello, Weavegraph workflow system!")
        .with_extra("numbers", json!([1, 2, 3]))
        .with_extra(
            "metadata",
            json!({
                "demo": "demo1",
                "stage": "initialization",
                "patterns": ["modern_messages", "state_builder"]
            }),
        )
        .build();

    info!("   ✓ Initial state created with builder pattern");
    info!(
        "   ✓ User message: {:?}",
        init.messages.snapshot()[0].content
    );
    info!(
        "   ✓ Extra data keys: {:?}",
        init.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // ✅ STEP 2: Modern Graph Building
    info!("\n🔗 Step 2: Building workflow graph with modern GraphBuilder");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("Initializer".into()),
            SimpleNode::new("Initializer"),
        )
        .add_node(
            NodeKind::Custom("ProcessorA".into()),
            SimpleNode::new("ProcessorA"),
        )
        .add_node(
            NodeKind::Custom("ProcessorB".into()),
            SimpleNode::new("ProcessorB"),
        )
        // Create a processing pipeline: Start -> A -> B -> End
        .add_edge(NodeKind::Start, NodeKind::Custom("Initializer".into()))
        .add_edge(
            NodeKind::Custom("Initializer".into()),
            NodeKind::Custom("ProcessorA".into()),
        )
        .add_edge(
            NodeKind::Custom("ProcessorA".into()),
            NodeKind::Custom("ProcessorB".into()),
        )
        .add_edge(NodeKind::Custom("ProcessorB".into()), NodeKind::End)
        // Add a secondary path: Start -> B (for demonstration of fan-out)
        .add_edge(NodeKind::Start, NodeKind::Custom("ProcessorB".into()))
        .compile()?;

    info!("   ✓ Graph compiled successfully");
    info!("   ✓ Nodes: Initializer, ProcessorA, ProcessorB, End");
    info!("   ✓ Edges: Start→A→B→End, Start→B (fan-out pattern)");

    // ✅ STEP 3: Full Workflow Execution
    info!("\n🚀 Step 3: Executing complete workflow");

    let final_state = app.invoke(init).await?;

    info!("   ✓ Workflow execution completed");
    let final_snapshot = final_state.snapshot();
    info!(
        "   ✓ Final message count: {}",
        final_snapshot.messages.len()
    );
    info!("   ✓ Messages version: {}", final_snapshot.messages_version);

    // Display the conversation flow
    info!("\n   📨 Message Flow:");
    for (i, msg) in final_snapshot.messages.iter().enumerate() {
        info!("      {}: [{}] {}", i + 1, msg.role, msg.content);
    }

    // ✅ STEP 4: State Snapshots and Mutations
    info!("\n📸 Step 4: Demonstrating state snapshots and mutations");

    let pre_mutation_snapshot = final_state.snapshot();
    info!(
        "   Pre-mutation: {} messages, {} extra keys",
        pre_mutation_snapshot.messages.len(),
        pre_mutation_snapshot.extra.len()
    );

    // Create a mutated copy to show immutability
    let mut mutated_state = final_state.clone();

    // ✅ MODERN: Use typed roles for message construction
    let post_run_message = Message::with_role(
        Role::Assistant,
        "This is a post-execution note added via mutation",
    );
    mutated_state.messages.get_mut().push(post_run_message);

    // Update version properly
    mutated_state
        .messages
        .set_version(pre_mutation_snapshot.messages_version.saturating_add(1));

    // Add extra metadata
    mutated_state.extra.get_mut().insert(
        "post_mutation".into(),
        json!({
            "added_at": "demo1",
            "operation": "mutation_demonstration"
        }),
    );

    let post_mutation_snapshot = mutated_state.snapshot();
    info!(
        "   Post-mutation: {} messages, {} extra keys",
        post_mutation_snapshot.messages.len(),
        post_mutation_snapshot.extra.len()
    );
    info!("   ✓ Original state remains unchanged (immutability preserved)");

    // ✅ STEP 5: Manual Barrier Operations
    info!("\n🚧 Step 5: Demonstrating manual barrier operations");

    let mut barrier_state = final_state.clone();

    // Create example NodePartials with modern message construction
    let mut extra_a = FxHashMap::default();
    extra_a.insert("source".into(), json!("manual_barrier_a"));
    extra_a.insert("priority".into(), json!("high"));

    let partial_a = NodePartial::new().with_messages(vec![Message::with_role(
        Role::Assistant,
        "Manual barrier message from virtual node A",
    )])
        .with_extra(extra_a);

    let mut extra_b = FxHashMap::default();
    extra_b.insert("source".into(), json!("manual_barrier_b"));
    extra_b.insert("priority".into(), json!("low")); // Will overwrite priority
    extra_b.insert("additional_data".into(), json!({"value": 42}));

    let partial_b = NodePartial::new().with_messages(vec![Message::with_role(
        Role::Assistant,
        "Manual barrier message from virtual node B",
    )])
        .with_extra(extra_b);

    let run_ids = vec![
        NodeKind::Custom("VirtualA".into()),
        NodeKind::Custom("VirtualB".into()),
    ];

    let barrier_outcome = app
        .apply_barrier(&mut barrier_state, &run_ids, vec![partial_a, partial_b])
        .await
        .map_err(|e| miette::miette!("Barrier operation failed: {e}"))?;

    info!("   ✓ Barrier applied successfully");
    info!(
        "   ✓ Updated channels: {:?}",
        barrier_outcome.updated_channels
    );
    info!(
        "   ✓ Errors recorded at barrier: {}",
        barrier_outcome.errors.len()
    );

    let barrier_snapshot = barrier_state.snapshot();
    info!(
        "   ✓ Messages after barrier: {}",
        barrier_snapshot.messages.len()
    );
    info!(
        "   ✓ Extra keys after barrier: {:?}",
        barrier_snapshot.extra.keys().collect::<Vec<_>>()
    );

    // Demonstrate no-op barrier (should not change versions)
    info!("\n   🔄 Testing no-op barrier operations");
    let pre_noop_version = barrier_state.messages.version();

    let noop_partials = vec![NodePartial::new().with_messages(vec![])]; // Empty - should not update

    let noop_outcome = app
        .apply_barrier(&mut barrier_state, &[], noop_partials)
        .await
        .map_err(|e| miette::miette!("No-op barrier failed: {e}"))?;

    let post_noop_version = barrier_state.messages.version();
    info!("   ✓ No-op barrier completed");
    info!(
        "   ✓ Version unchanged: {} -> {} (expected same)",
        pre_noop_version, post_noop_version
    );
    info!("   ✓ Updated channels: {:?}", noop_outcome.updated_channels);
    info!(
        "   ✓ Errors recorded at barrier: {}",
        noop_outcome.errors.len()
    );

    // ✅ STEP 6: Error Handling Demonstrations
    info!("\n❌ Step 6: Demonstrating error handling and validation");

    // (Removed obsolete test: entry point validation no longer enforced. Start/End are virtual.)
    info!("   🧪 Skipping deprecated entry-point error test (Start/End now virtual).");

    // Test 3: Version saturation behavior
    info!("   🧪 Test 3: Version saturation behavior");
    let mut saturation_state = final_state.clone();
    saturation_state.messages.set_version(u32::MAX);

    let saturation_partial = NodePartial::new().with_messages(vec![Message::with_role(
        Role::Assistant,
        "This message won't increment version due to saturation",
    )]);

    let pre_saturation_version = saturation_state.messages.version();
    let _ = app
        .apply_barrier(&mut saturation_state, &[], vec![saturation_partial])
        .await
        .map_err(|e| miette::miette!("Saturation test failed: {e}"))?;

    let post_saturation_version = saturation_state.messages.version();
    info!("   ✓ Version saturation test completed");
    info!(
        "   ✓ Version remained at MAX: {} -> {} (expected same)",
        pre_saturation_version, post_saturation_version
    );

    // ✅ FINAL SUMMARY
    info!("\n╔══════════════════════════════════════════════════════════╗");
    info!("║                      Demo 1 Complete                    ║");
    info!("╚══════════════════════════════════════════════════════════╝");
    info!("\n✅ Key patterns demonstrated:");
    info!("   • Modern message construction with typed roles");
    info!("   • State building with fluent builder pattern");
    info!("   • Graph compilation and execution");
    info!("   • State snapshots and mutation safety");
    info!("   • Manual barrier operations");
    info!("   • Error handling and validation");
    info!("\n🎯 Next: Run demo2 to see scheduler-driven execution patterns");

    Ok(())
}
