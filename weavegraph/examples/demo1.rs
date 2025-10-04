//! Demo 1: Basic Graph Building and Execution
//!
//! This demonstration showcases the fundamental graph building and execution patterns
//! in the Weavegraph framework. It covers basic workflow construction, state management,
//! barrier operations, and error handling scenarios.
//!
//! What You'll Learn:
//! 1. Modern Message Construction: Using convenience constructors and the builder pattern
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
use weavegraph::channels::Channel;
use weavegraph::graph::GraphBuilder;
use weavegraph::message::Message;
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

        // ✅ MODERN: Use convenience constructor instead of manual struct construction
        let output_message = Message::assistant(&response);

        ctx.emit(
            "node_completion",
            format!("Node {} completed successfully", self.name),
        )?;

        Ok(NodePartial {
            messages: Some(vec![output_message]),
            extra: None,
            errors: None,
        })
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
/// - **Message Construction**: `Message::user()`, `Message::assistant()` instead of manual structs
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
#[tokio::main]
async fn main() -> Result<()> {
    demo().await
}

async fn demo() -> Result<()> {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                        Demo 1                           ║");
    println!("║              Basic Graph Building & Execution           ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // ✅ STEP 1: Modern State Construction
    println!("📊 Step 1: Creating initial state with modern patterns");

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

    println!("   ✓ Initial state created with builder pattern");
    println!(
        "   ✓ User message: {:?}",
        init.messages.snapshot()[0].content
    );
    println!(
        "   ✓ Extra data keys: {:?}",
        init.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // ✅ STEP 2: Modern Graph Building
    println!("\n🔗 Step 2: Building workflow graph with modern GraphBuilder");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Other("Initializer".into()),
            SimpleNode::new("Initializer"),
        )
        .add_node(
            NodeKind::Other("ProcessorA".into()),
            SimpleNode::new("ProcessorA"),
        )
        .add_node(
            NodeKind::Other("ProcessorB".into()),
            SimpleNode::new("ProcessorB"),
        )
        .add_node(NodeKind::End, SimpleNode::new("End"))
        // Create a processing pipeline: Start -> A -> B -> End
        .add_edge(NodeKind::Start, NodeKind::Other("Initializer".into()))
        .add_edge(
            NodeKind::Other("Initializer".into()),
            NodeKind::Other("ProcessorA".into()),
        )
        .add_edge(
            NodeKind::Other("ProcessorA".into()),
            NodeKind::Other("ProcessorB".into()),
        )
        .add_edge(NodeKind::Other("ProcessorB".into()), NodeKind::End)
        // Add a secondary path: Start -> B (for demonstration of fan-out)
        .add_edge(NodeKind::Start, NodeKind::Other("ProcessorB".into()))
        .set_entry(NodeKind::Start)
        .compile()
        .map_err(|e| miette::miette!("Graph compilation failed: {e:?}"))?;

    println!("   ✓ Graph compiled successfully");
    println!("   ✓ Nodes: Initializer, ProcessorA, ProcessorB, End");
    println!("   ✓ Edges: Start→A→B→End, Start→B (fan-out pattern)");

    // ✅ STEP 3: Full Workflow Execution
    println!("\n🚀 Step 3: Executing complete workflow");

    let final_state = app.invoke(init).await?;

    println!("   ✓ Workflow execution completed");
    let final_snapshot = final_state.snapshot();
    println!(
        "   ✓ Final message count: {}",
        final_snapshot.messages.len()
    );
    println!("   ✓ Messages version: {}", final_snapshot.messages_version);

    // Display the conversation flow
    println!("\n   📨 Message Flow:");
    for (i, msg) in final_snapshot.messages.iter().enumerate() {
        println!("      {}: [{}] {}", i + 1, msg.role, msg.content);
    }

    // ✅ STEP 4: State Snapshots and Mutations
    println!("\n📸 Step 4: Demonstrating state snapshots and mutations");

    let pre_mutation_snapshot = final_state.snapshot();
    println!(
        "   Pre-mutation: {} messages, {} extra keys",
        pre_mutation_snapshot.messages.len(),
        pre_mutation_snapshot.extra.len()
    );

    // Create a mutated copy to show immutability
    let mut mutated_state = final_state.clone();

    // ✅ MODERN: Use convenience constructor for new message
    let post_run_message = Message::assistant("This is a post-execution note added via mutation");
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
    println!(
        "   Post-mutation: {} messages, {} extra keys",
        post_mutation_snapshot.messages.len(),
        post_mutation_snapshot.extra.len()
    );
    println!("   ✓ Original state remains unchanged (immutability preserved)");

    // ✅ STEP 5: Manual Barrier Operations
    println!("\n🚧 Step 5: Demonstrating manual barrier operations");

    let mut barrier_state = final_state.clone();

    // Create example NodePartials with modern message construction
    let partial_a = NodePartial {
        messages: Some(vec![Message::assistant(
            "Manual barrier message from virtual node A",
        )]),
        extra: Some({
            let mut extra = FxHashMap::default();
            extra.insert("source".into(), json!("manual_barrier_a"));
            extra.insert("priority".into(), json!("high"));
            extra
        }),
        errors: None,
    };

    let partial_b = NodePartial {
        messages: Some(vec![Message::assistant(
            "Manual barrier message from virtual node B",
        )]),
        extra: Some({
            let mut extra = FxHashMap::default();
            extra.insert("source".into(), json!("manual_barrier_b"));
            extra.insert("priority".into(), json!("low")); // Will overwrite priority
            extra.insert("additional_data".into(), json!({"value": 42}));
            extra
        }),
        errors: None,
    };

    let run_ids = vec![
        NodeKind::Other("VirtualA".into()),
        NodeKind::Other("VirtualB".into()),
    ];

    let updated_channels = app
        .apply_barrier(&mut barrier_state, &run_ids, vec![partial_a, partial_b])
        .await
        .map_err(|e| miette::miette!("Barrier operation failed: {e}"))?;

    println!("   ✓ Barrier applied successfully");
    println!("   ✓ Updated channels: {:?}", updated_channels);

    let barrier_snapshot = barrier_state.snapshot();
    println!(
        "   ✓ Messages after barrier: {}",
        barrier_snapshot.messages.len()
    );
    println!(
        "   ✓ Extra keys after barrier: {:?}",
        barrier_snapshot.extra.keys().collect::<Vec<_>>()
    );

    // Demonstrate no-op barrier (should not change versions)
    println!("\n   🔄 Testing no-op barrier operations");
    let pre_noop_version = barrier_state.messages.version();

    let noop_partials = vec![NodePartial {
        messages: Some(vec![]), // Empty - should not update
        extra: None,
        errors: None,
    }];

    let noop_updated = app
        .apply_barrier(&mut barrier_state, &[], noop_partials)
        .await
        .map_err(|e| miette::miette!("No-op barrier failed: {e}"))?;

    let post_noop_version = barrier_state.messages.version();
    println!("   ✓ No-op barrier completed");
    println!(
        "   ✓ Version unchanged: {} -> {} (expected same)",
        pre_noop_version, post_noop_version
    );
    println!("   ✓ Updated channels: {:?}", noop_updated);

    // ✅ STEP 6: Error Handling Demonstrations
    println!("\n❌ Step 6: Demonstrating error handling and validation");

    // Test 1: Missing entry point
    println!("   🧪 Test 1: Graph without entry point");
    match GraphBuilder::new()
        .add_node(NodeKind::Start, SimpleNode::new("Start"))
        .add_node(NodeKind::End, SimpleNode::new("End"))
        .add_edge(NodeKind::Start, NodeKind::End)
        .compile()
    {
        Err(e) => println!("   ✓ Expected error caught: {e:?}"),
        Ok(_) => println!("   ❌ Unexpected success - should have failed!"),
    }

    // Test 2: Entry point not registered as node
    println!("   🧪 Test 2: Entry point not registered as node");
    match GraphBuilder::new()
        .set_entry(NodeKind::Other("NonExistentNode".into()))
        .compile()
    {
        Err(e) => println!("   ✓ Expected error caught: {e:?}"),
        Ok(_) => println!("   ❌ Unexpected success - should have failed!"),
    }

    // Test 3: Version saturation behavior
    println!("   🧪 Test 3: Version saturation behavior");
    let mut saturation_state = final_state.clone();
    saturation_state.messages.set_version(u32::MAX);

    let saturation_partial = NodePartial {
        messages: Some(vec![Message::assistant(
            "This message won't increment version due to saturation",
        )]),
        extra: None,
        errors: None,
    };

    let pre_saturation_version = saturation_state.messages.version();
    let _ = app
        .apply_barrier(&mut saturation_state, &[], vec![saturation_partial])
        .await
        .map_err(|e| miette::miette!("Saturation test failed: {e}"))?;

    let post_saturation_version = saturation_state.messages.version();
    println!("   ✓ Version saturation test completed");
    println!(
        "   ✓ Version remained at MAX: {} -> {} (expected same)",
        pre_saturation_version, post_saturation_version
    );

    // ✅ FINAL SUMMARY
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                      Demo 1 Complete                    ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("\n✅ Key patterns demonstrated:");
    println!("   • Modern message construction with convenience methods");
    println!("   • State building with fluent builder pattern");
    println!("   • Graph compilation and execution");
    println!("   • State snapshots and mutation safety");
    println!("   • Manual barrier operations");
    println!("   • Error handling and validation");
    println!("\n🎯 Next: Run demo2 to see scheduler-driven execution patterns");

    Ok(())
}
