//! Demo 2: Scheduler-Driven Workflow Execution
//!
//! This example demonstrates the advanced scheduler-driven execution model in Weavegraph.
//! Unlike the basic graph execution in demo1, this shows how to:
//!
//! 1. Create nodes with variable execution times for scheduling demonstration
//! 2. Build complex dependency graphs with fan-out and convergence
//! 3. Execute workflows with proper dependency resolution
//! 4. Track execution timing and concurrency patterns
//!
//! The scheduler ensures nodes only execute when their dependencies are ready,
//! providing efficient concurrent execution while respecting the dependency graph.

use async_trait::async_trait;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::time::Duration;
use tokio::time::sleep;
use weavegraph::channels::Channel;
use weavegraph::graph::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

/// A demonstration node that simulates variable execution time
/// to showcase scheduler behavior with dependencies
#[derive(Debug, Clone)]
struct SchedulerDemoNode {
    name: String,
    execution_time_ms: u64,
}

impl SchedulerDemoNode {
    fn new(name: &str, execution_time_ms: u64) -> Self {
        Self {
            name: name.to_string(),
            execution_time_ms,
        }
    }
}

#[async_trait]
impl Node for SchedulerDemoNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        // Emit execution start event
        ctx.emit(
            "scheduler_node_start",
            format!(
                "Node {} starting execution ({}ms)",
                self.name, self.execution_time_ms
            ),
        )?;

        println!(
            "   🔄 [{}] Starting execution ({}ms simulation)",
            self.name, self.execution_time_ms
        );

        // Simulate processing time to demonstrate scheduler behavior
        sleep(Duration::from_millis(self.execution_time_ms)).await;

        let input_msg = snapshot
            .messages
            .last()
            .map(|msg| msg.content.clone())
            .unwrap_or_else(|| "No input message".to_string());

        // Create response based on node type
        let response_content = match self.name.as_str() {
            "Start" => format!("🚀 Workflow initiated: {}", input_msg),
            "Analyzer" => format!("📊 Analysis complete: Processed '{}'", input_msg),
            "ProcessorA" => format!("⚙️ ProcessorA: Transformed '{}'", input_msg),
            "ProcessorB" => format!("⚙️ ProcessorB: Enhanced '{}'", input_msg),
            "Synthesizer" => "🔄 Synthesis: Combined all inputs into final result".to_string(),
            "End" => "✅ Workflow completed successfully".to_string(),
            _ => format!("🔄 [{}] Processed: {}", self.name, input_msg),
        };

        println!(
            "   ✅ [{}] Completed after {}ms",
            self.name, self.execution_time_ms
        );

        // Emit completion event
        ctx.emit(
            "scheduler_node_complete",
            format!(
                "Node {} completed after {}ms",
                self.name, self.execution_time_ms
            ),
        )?;

        let result_message = Message::assistant(&response_content);

        // Add execution metadata
        let mut extra = FxHashMap::default();
        extra.insert("node_name".into(), json!(self.name));
        extra.insert("execution_time_ms".into(), json!(self.execution_time_ms));
        extra.insert("timestamp".into(), json!(chrono::Utc::now().to_rfc3339()));

        Ok(NodePartial {
            messages: Some(vec![result_message]),
            extra: Some(extra),
            errors: None,
        })
    }
}

/// Main demonstration function showing scheduler-driven execution
async fn run_demo2() -> miette::Result<()> {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                        Demo 2                           ║");
    println!("║         Scheduler-Driven Workflow Execution             ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // ✅ STEP 1: Modern State Construction with Rich Context
    println!("📊 Step 1: Creating initial state for scheduler demonstration");

    let init = VersionedState::builder()
        .with_user_message(
            "Analyze the performance characteristics of concurrent workflow execution",
        )
        .with_extra("execution_mode", json!("scheduler_driven"))
        .with_extra(
            "concurrency_config",
            json!({
                "max_parallel_nodes": 2,
                "enable_event_streaming": true,
                "track_execution_timing": true
            }),
        )
        .with_extra(
            "analysis_parameters",
            json!({
                "focus_areas": ["concurrency", "dependency_resolution", "performance"],
                "detail_level": "verbose"
            }),
        )
        .build();

    println!("   ✓ Rich initial state created");
    println!("   ✓ User query: {}", init.messages.snapshot()[0].content);
    println!(
        "   ✓ Configuration keys: {:?}",
        init.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // ✅ STEP 2: Building a Complex Graph for Scheduler Demo
    println!("\n🔗 Step 2: Building complex graph with dependencies and fan-out");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Other("Initializer".into()),
            SchedulerDemoNode::new("Initializer", 50),
        )
        .add_node(
            NodeKind::Other("Analyzer".into()),
            SchedulerDemoNode::new("Analyzer", 200),
        )
        .add_node(
            NodeKind::Other("ProcessorA".into()),
            SchedulerDemoNode::new("ProcessorA", 150),
        )
        .add_node(
            NodeKind::Other("ProcessorB".into()),
            SchedulerDemoNode::new("ProcessorB", 100),
        )
        .add_node(
            NodeKind::Other("Synthesizer".into()),
            SchedulerDemoNode::new("Synthesizer", 300),
        )
        .add_node(NodeKind::End, SchedulerDemoNode::new("End", 75))
        // Create complex dependency graph:
        // Start fans out to Analyzer and ProcessorA
        .add_edge(NodeKind::Start, NodeKind::Other("Initializer".into()))
        .add_edge(
            NodeKind::Other("Initializer".into()),
            NodeKind::Other("Analyzer".into()),
        )
        .add_edge(
            NodeKind::Other("Initializer".into()),
            NodeKind::Other("ProcessorA".into()),
        )
        // Analyzer feeds into ProcessorB
        .add_edge(
            NodeKind::Other("Analyzer".into()),
            NodeKind::Other("ProcessorB".into()),
        )
        // Both ProcessorA and ProcessorB feed into Synthesizer
        .add_edge(
            NodeKind::Other("ProcessorA".into()),
            NodeKind::Other("Synthesizer".into()),
        )
        .add_edge(
            NodeKind::Other("ProcessorB".into()),
            NodeKind::Other("Synthesizer".into()),
        )
        // Synthesizer feeds into End
        .add_edge(NodeKind::Other("Synthesizer".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .compile()
        .map_err(|e| miette::miette!("Graph compilation failed: {e:?}"))?;

    println!("   ✓ Complex graph compiled successfully");
    println!("   ✓ Nodes: Start → [Analyzer, ProcessorA] → ProcessorB → Synthesizer → End");
    println!("   ✓ Dependencies: Multiple fan-out and convergence points");

    // ✅ STEP 3: Execute the workflow
    println!("\n🚀 Step 3: Executing scheduler-driven workflow");
    println!("   🕐 Watch the timing - dependencies control execution order");

    let start_time = std::time::Instant::now();
    let final_state = app.invoke(init).await?;
    let total_time = start_time.elapsed();

    println!("\n✅ Demo 2 completed - Scheduler execution successful!");
    println!("   ⏱️  Total execution time: {:?}", total_time);
    println!(
        "   📨 Final messages: {}",
        final_state.messages.snapshot().len()
    );
    println!("   ✓ Complex dependency graph executed");
    println!("   ✓ Concurrency control demonstrated");

    // Show the message flow
    println!("\n   📨 Execution Flow:");
    for (i, msg) in final_state.messages.snapshot().iter().enumerate() {
        println!("      {}: [{}] {}", i + 1, msg.role, msg.content);
    }

    Ok(())
}

#[tokio::main]
async fn main() -> miette::Result<()> {
    run_demo2().await
}
