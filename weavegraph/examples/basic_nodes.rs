//! Basic node examples demonstrating core patterns.
//!
//! This example shows how to create simple nodes that:
//! - Process workflow state
//! - Emit events for observability
//! - Return partial state updates
//! - Use convenience constructors
//!
//! Run with: `cargo run --example basic_nodes`

use async_trait::async_trait;
use miette::Result;
use serde_json::json;
use std::sync::Arc;
use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::fmt::format::FmtSpan;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt};
use weavegraph::channels::Channel;
use weavegraph::event_bus::EventBus;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::utils::collections::new_extra_map;

/// A simple message processing node that counts and logs messages.
///
/// This demonstrates:
/// - Reading from the current state
/// - Emitting progress events
/// - Adding messages and extra data
/// - Using convenience constructors
pub struct MessageCounterNode {
    pub node_name: String,
}

#[async_trait]
impl Node for MessageCounterNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let message_count = snapshot.messages.len();

        ctx.emit(
            "processing",
            format!(
                "{} starting (found {} existing messages)",
                self.node_name, message_count
            ),
        )?;

        // Create a summary message
        let summary = format!(
            "{} processed {} messages at step {}",
            self.node_name, message_count, ctx.step
        );

        // Store metadata about our processing
        let mut extra = new_extra_map();
        extra.insert("processor".to_string(), json!(self.node_name));
        extra.insert("message_count".to_string(), json!(message_count));
        extra.insert("step".to_string(), json!(ctx.step));
        extra.insert(
            "timestamp".to_string(),
            json!(chrono::Utc::now().to_rfc3339()),
        );

        ctx.emit(
            "completed",
            format!("{} finished processing", self.node_name),
        )?;

        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(Role::Assistant, &summary)])
            .with_extra(extra))
    }
}

/// A validation node that checks for required data.
///
/// This demonstrates:
/// - Input validation patterns
/// - Conditional logic based on state
/// - Error handling (both fatal and recoverable)
/// - Reading data from previous nodes
pub struct ValidationNode {
    pub required_fields: Vec<String>,
    pub min_message_count: usize,
}

#[async_trait]
impl Node for ValidationNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("validation", "Starting input validation")?;

        // Check message count
        if snapshot.messages.len() < self.min_message_count {
            return Err(NodeError::ValidationFailed(format!(
                "Expected at least {} messages, found {}",
                self.min_message_count,
                snapshot.messages.len()
            )));
        }

        // Check for required fields in extra data
        let mut missing_fields = Vec::new();
        for field in &self.required_fields {
            if !snapshot.extra.contains_key(field) {
                missing_fields.push(field.clone());
            }
        }

        if !missing_fields.is_empty() {
            return Err(NodeError::ValidationFailed(format!(
                "Missing required fields: {}",
                missing_fields.join(", ")
            )));
        }

        ctx.emit("validation", "All validations passed")?;

        // Return validation results as extra data
        let mut extra = new_extra_map();
        extra.insert("validation_status".to_string(), json!("passed"));
        extra.insert("validated_fields".to_string(), json!(self.required_fields));
        extra.insert("message_count_ok".to_string(), json!(true));

        Ok(NodePartial::new().with_extra(extra))
    }
}

/// A data aggregation node that summarizes previous processing.
///
/// This demonstrates:
/// - Reading complex data from previous nodes
/// - Data transformation and aggregation
/// - Conditional warnings
/// - Rich metadata storage
pub struct AggregatorNode;

#[async_trait]
impl Node for AggregatorNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("aggregation", "Starting data aggregation")?;

        // Aggregate data from previous nodes
        let mut processors = Vec::new();
        let mut total_steps = 0u64;

        for (key, value) in &snapshot.extra {
            if key == "processor"
                && let Some(processor_name) = value.as_str()
            {
                processors.push(processor_name.to_string());
            }
            if key == "step"
                && let Some(step) = value.as_u64()
            {
                total_steps += step;
            }
        }

        // Check for potential issues
        if total_steps > 100 {
            ctx.emit(
                "warning",
                format!(
                    "Total processing steps ({}) exceeds recommended threshold",
                    total_steps
                ),
            )?;
        }

        let summary = format!(
            "Aggregated data from {} processors across {} total steps",
            processors.len(),
            total_steps
        );

        let mut extra = new_extra_map();
        extra.insert(
            "aggregation_summary".to_string(),
            json!({
                "processors": processors,
                "total_steps": total_steps,
                "message_count": snapshot.messages.len(),
                "aggregated_at": chrono::Utc::now().to_rfc3339()
            }),
        );

        ctx.emit("completed", "Data aggregation completed")?;

        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(Role::Assistant, &summary)])
            .with_extra(extra))
    }
}

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

    info!("🔧 Basic Node Examples");
    info!("======================");

    // Set up event bus for observability
    let event_bus = EventBus::default();

    // Start listening for events in the background
    event_bus.listen_for_events();

    // Create initial state
    let mut state = VersionedState::builder()
        .with_user_message("Initial user message")
        .with_extra("processor", json!("initial"))
        .with_extra("step", json!(1))
        .build();

    info!("\n📊 Initial State:");
    info!("  Messages: {}", state.messages.snapshot().len());
    info!(
        "  Extra keys: {:?}",
        state.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // Run MessageCounterNode
    info!("\n🔄 Running MessageCounterNode...");
    let counter_node = MessageCounterNode {
        node_name: "CounterExample".to_string(),
    };

    let emitter = event_bus.get_emitter();

    let ctx1 = NodeContext {
        node_id: "counter-1".to_string(),
        step: 2,
        event_emitter: Arc::clone(&emitter),
    };

    let result1 = counter_node.run(state.snapshot(), ctx1).await?;

    // Apply the result (simulating runtime behavior)
    if let Some(messages) = result1.messages {
        state.messages.get_mut().extend(messages);
    }
    if let Some(extra) = result1.extra {
        state.extra.get_mut().extend(extra);
    }

    info!("  ✅ Messages now: {}", state.messages.snapshot().len());
    info!(
        "  ✅ Extra keys: {:?}",
        state.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // Run ValidationNode
    info!("\n🔍 Running ValidationNode...");
    let validation_node = ValidationNode {
        required_fields: vec!["processor".to_string(), "step".to_string()],
        min_message_count: 1,
    };

    let ctx2 = NodeContext {
        node_id: "validator-1".to_string(),
        step: 3,
        event_emitter: Arc::clone(&emitter),
    };

    let result2 = validation_node.run(state.snapshot(), ctx2).await?;

    if let Some(extra) = result2.extra {
        state.extra.get_mut().extend(extra);
    }

    info!("  ✅ Validation passed");

    // Run AggregatorNode
    info!("\n📈 Running AggregatorNode...");
    let aggregator_node = AggregatorNode;

    let ctx3 = NodeContext {
        node_id: "aggregator-1".to_string(),
        step: 4,
        event_emitter: Arc::clone(&emitter),
    };

    let result3 = aggregator_node.run(state.snapshot(), ctx3).await?;

    if let Some(messages) = result3.messages {
        state.messages.get_mut().extend(messages);
    }
    if let Some(extra) = result3.extra {
        state.extra.get_mut().extend(extra);
    }

    info!("  ✅ Aggregation completed");

    // Display final state
    info!("\n📋 Final State:");
    let final_snapshot = state.snapshot();

    info!("  Messages: {}", final_snapshot.messages.len());
    for (i, msg) in final_snapshot.messages.iter().enumerate() {
        info!("    {}: [{}] {}", i + 1, msg.role, msg.content);
    }
    info!(
        "  Extra data keys: {:?}",
        final_snapshot.extra.keys().collect::<Vec<_>>()
    );

    // Give a moment for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // Stop the event listener
    event_bus.stop_listener().await;

    info!("\n✅ Example completed successfully!");
    Ok(())
}
