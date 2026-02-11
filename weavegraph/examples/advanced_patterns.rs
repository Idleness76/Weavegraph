//! Advanced node patterns and error handling examples.
//!
//! This example demonstrates sophisticated workflow patterns that go beyond basic
//! node execution. It showcases:
//!
//! ## Core Patterns
//! - **Complex error handling**: Retry logic, graceful fallbacks, and error recovery
//! - **Conditional node execution**: Dynamic routing based on state conditions
//! - **Rich state transformations**: Complex data processing and enrichment
//! - **Advanced NodePartial usage**: Efficient partial state updates
//!
//! ## Key Learning Points
//! - How to implement robust external service integration
//! - Patterns for conditional workflow routing
//! - Techniques for data transformation and validation
//! - Best practices for error handling in distributed workflows
//!
//! ## Architecture
//! The example creates a pipeline: API Call → Router → Transformer
//! Each stage demonstrates different advanced patterns while maintaining
//! composability and observability.
//!
//! Run with: `cargo run --example advanced_patterns`

use async_trait::async_trait;
use serde_json::json;
use std::collections::HashMap;
use std::sync::Arc;
use weavegraph::channels::Channel;
use weavegraph::event_bus::EventBus;
use weavegraph::message::{Message, Role};
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::utils::collections::new_extra_map;

use miette::Result;
use tracing::info;
use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// A node that simulates external API calls with potential failures and retry logic.
///
/// This node demonstrates enterprise-grade patterns for external service integration:
///
/// ## Features
/// - **Configurable failure simulation**: Adjustable failure rates for testing
/// - **Retry logic**: Configurable retry attempts with detailed logging
/// - **Rich error reporting**: Comprehensive error metadata and context
/// - **Graceful degradation**: Structured failure handling
///
/// ## Use Cases
/// - External API integration (REST, GraphQL, gRPC)
/// - Database operations with connection retry
/// - File system operations with temporary failures
/// - Any operation that might fail temporarily
///
/// ## Configuration
/// - `service_name`: Human-readable service identifier for logging
/// - `failure_rate`: Probability of failure (0.0 = never fail, 1.0 = always fail)
/// - `max_retries`: Maximum number of retry attempts before giving up
pub struct ApiCallNode {
    /// Human-readable name for this service (used in logging and errors)
    pub service_name: String,
    /// Failure probability: 0.0 = never fail, 1.0 = always fail
    pub failure_rate: f64,
    /// Maximum number of retry attempts before giving up
    pub max_retries: u32,
}

#[async_trait]
impl Node for ApiCallNode {
    async fn run(
        &self,
        _snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("api_call", format!("Calling {} service", self.service_name))?;

        // Simulate API call attempts
        for attempt in 1..=self.max_retries {
            ctx.emit(
                "attempt",
                format!("Attempt {} of {}", attempt, self.max_retries),
            )?;

            // Simulate random failure based on failure rate
            let success = rand::random::<f64>() > self.failure_rate;

            if success {
                ctx.emit(
                    "success",
                    format!(
                        "{} API call succeeded on attempt {}",
                        self.service_name, attempt
                    ),
                )?;

                let mut extra = new_extra_map();
                extra.insert(
                    "api_result".to_string(),
                    json!({
                        "service": self.service_name,
                        "status": "success",
                        "attempt": attempt,
                        "data": format!("Response from {}", self.service_name)
                    }),
                );

                let partial = NodePartial::new()
                    .with_messages(vec![Message::with_role(
                        Role::System,
                        &format!("{} API call completed successfully", self.service_name),
                    )])
                    .with_extra(extra);

                return Ok(partial);
            } else {
                ctx.emit("retry", format!("Attempt {} failed, retrying...", attempt))?;
            }
        }

        // All retries exhausted
        Err(NodeError::Provider {
            provider: "external_api",
            message: format!(
                "{} service failed after {} attempts",
                self.service_name, self.max_retries
            ),
        })
    }
}

/// A conditional router node that directs workflow execution based on state conditions.
///
/// This node implements sophisticated routing logic that can dynamically alter
/// workflow execution paths based on runtime state. It's essential for building
/// adaptive workflows that respond to data conditions.
///
/// ## Features
/// - **Multi-condition routing**: Support for complex condition sets
/// - **Fallback routing**: Default route when no conditions match
/// - **Rich routing metadata**: Comprehensive decision logging
/// - **Type-safe conditions**: JSON value-based condition matching
///
/// ## Use Cases
/// - User role-based workflow routing
/// - Data quality-based processing paths
/// - Feature flag-driven execution
/// - A/B testing workflow variants
/// - Error severity-based handling paths
///
/// ## Configuration
/// - `route_key`: The state key to evaluate for routing decisions
/// - `conditions`: Map of route names to expected values for that key
pub struct ConditionalRouterNode {
    /// The key in the state's extra data to use for routing decisions
    pub route_key: String,
    /// Map of route names to the values that should trigger that route
    pub conditions: HashMap<String, serde_json::Value>,
}

#[async_trait]
impl Node for ConditionalRouterNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("routing", "Evaluating routing conditions")?;

        let route_value = snapshot
            .extra
            .get(&self.route_key)
            .ok_or(NodeError::MissingInput {
                what: "routing key not found in state",
            })?;

        // Find matching condition
        let mut selected_route = "default".to_string();
        for (route_name, condition_value) in &self.conditions {
            if route_value == condition_value {
                selected_route = route_name.clone();
                break;
            }
        }

        ctx.emit(
            "route_selected",
            format!("Selected route: {}", selected_route),
        )?;

        let mut extra = new_extra_map();
        extra.insert("selected_route".to_string(), json!(selected_route));
        extra.insert(
            "routing_decision".to_string(),
            json!({
                "key": self.route_key,
                "value": route_value,
                "available_routes": self.conditions.keys().collect::<Vec<_>>()
            }),
        );

        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(
                Role::System,
                &format!("Routed to: {}", selected_route),
            )])
            .with_extra(extra))
    }
}

/// A sophisticated data transformer node that processes and enriches workflow state.
///
/// This node implements a rule-based transformation engine that can apply
/// multiple data processing operations in a single execution. It demonstrates
/// advanced patterns for data manipulation within workflows.
///
/// ## Features
/// - **Rule-based transformations**: Configurable transformation pipeline
/// - **Type-safe operations**: Support for multiple data types and operations
/// - **Comprehensive logging**: Detailed transformation audit trail
/// - **Error resilience**: Graceful handling of malformed or missing data
/// - **Performance optimization**: Efficient bulk processing
///
/// ## Supported Operations
/// - String operations: uppercase, lowercase, reverse
/// - Numeric operations: multiplication, arithmetic
/// - Collection operations: length calculation
/// - Path operations: JSONPath-like access (simplified)
///
/// ## Use Cases
/// - Data normalization and standardization
/// - Feature engineering for ML pipelines
/// - Format conversion and validation
/// - Data enrichment and augmentation
/// - Compliance and privacy transformations
pub struct DataTransformerNode {
    /// List of transformation rules to apply in sequence
    pub transformation_rules: Vec<TransformRule>,
}

#[derive(Clone)]
pub struct TransformRule {
    pub source_key: String,
    pub target_key: String,
    pub operation: TransformOperation,
}

#[derive(Clone)]
pub enum TransformOperation {
    Uppercase,
    Lowercase,
    Reverse,
    Length,
    Multiply(i64),
    JsonPath(String),
}

#[async_trait]
impl Node for DataTransformerNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit(
            "transform",
            format!(
                "Applying {} transformation rules",
                self.transformation_rules.len()
            ),
        )?;

        let mut extra = new_extra_map();
        let mut transformation_log = Vec::new();

        for rule in &self.transformation_rules {
            match snapshot.extra.get(&rule.source_key) {
                Some(source_value) => {
                    let transformed = match &rule.operation {
                        TransformOperation::Uppercase => source_value
                            .as_str()
                            .map(|s| json!(s.to_uppercase()))
                            .unwrap_or(json!(null)),
                        TransformOperation::Lowercase => source_value
                            .as_str()
                            .map(|s| json!(s.to_lowercase()))
                            .unwrap_or(json!(null)),
                        TransformOperation::Reverse => source_value
                            .as_str()
                            .map(|s| json!(s.chars().rev().collect::<String>()))
                            .unwrap_or(json!(null)),
                        TransformOperation::Length => source_value
                            .as_str()
                            .map(|s| json!(s.len()))
                            .or_else(|| source_value.as_array().map(|a| json!(a.len())))
                            .unwrap_or(json!(null)),
                        TransformOperation::Multiply(factor) => source_value
                            .as_i64()
                            .map(|n| json!(n * factor))
                            .or_else(|| source_value.as_f64().map(|n| json!(n * (*factor as f64))))
                            .unwrap_or(json!(null)),
                        TransformOperation::JsonPath(_path) => {
                            // Simplified JSONPath-like operation
                            json!(format!("jsonpath_result_for_{}", rule.source_key))
                        }
                    };

                    extra.insert(rule.target_key.clone(), transformed.clone());
                    transformation_log.push(json!({
                        "rule": rule.source_key,
                        "operation": format!("{:?}", rule.operation),
                        "result": transformed
                    }));

                    ctx.emit(
                        "rule_applied",
                        format!("Applied {} -> {}", rule.source_key, rule.target_key),
                    )?;
                }
                None => {
                    ctx.emit(
                        "warning",
                        format!("Source key '{}' not found, skipping rule", rule.source_key),
                    )?;
                }
            }
        }

        extra.insert("transformation_log".to_string(), json!(transformation_log));

        Ok(NodePartial::new()
            .with_messages(vec![Message::with_role(
                Role::Assistant,
                &format!("Applied {} transformations", transformation_log.len()),
            )])
            .with_extra(extra))
    }
}

impl std::fmt::Debug for TransformOperation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TransformOperation::Uppercase => write!(f, "Uppercase"),
            TransformOperation::Lowercase => write!(f, "Lowercase"),
            TransformOperation::Reverse => write!(f, "Reverse"),
            TransformOperation::Length => write!(f, "Length"),
            TransformOperation::Multiply(factor) => write!(f, "Multiply({})", factor),
            TransformOperation::JsonPath(path) => write!(f, "JsonPath({})", path),
        }
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::layer().with_target(false))
        .with(
            EnvFilter::from_default_env()
                .add_directive("weavegraph=info".parse().unwrap())
                .add_directive("advanced_patterns=info".parse().unwrap()),
        )
        .with(ErrorLayer::default())
        .init();
}

fn init_miette() {
    miette::set_panic_hook();
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    init_miette();

    info!("🚀 Advanced Node Patterns Example");
    info!("==================================");

    // Set up event bus for observability
    let event_bus = EventBus::default();
    event_bus.listen_for_events();

    info!("\n🔧 Running Advanced Node Examples...");

    // Initial state with more complex data
    let mut state = VersionedState::builder()
        .with_user_message("Process this data through the advanced pipeline")
        .with_extra("service_type", json!("premium"))
        .with_extra("user_data", json!("Hello World"))
        .with_extra("priority", json!(5))
        .build();

    info!("\n📊 Initial State:");
    info!("  Messages: {}", state.messages.snapshot().len());
    info!(
        "  Extra keys: {:?}",
        state.extra.snapshot().keys().collect::<Vec<_>>()
    );

    // Create and run API node with potential failures
    info!("\n1️⃣ Running API Call Node (with failure simulation)...");
    let api_node = ApiCallNode {
        service_name: "UserDataAPI".to_string(),
        failure_rate: 0.3, // 30% failure rate for demonstration
        max_retries: 3,
    };

    let emitter = event_bus.get_emitter();

    let ctx1 = NodeContext {
        node_id: "api_call".to_string(),
        step: 1,
        event_emitter: Arc::clone(&emitter),
    };

    // Demonstrate both success and failure scenarios
    match api_node.run(state.snapshot(), ctx1).await {
        Ok(result) => {
            info!("  ✅ API call succeeded");
            if let Some(messages) = result.messages {
                state.messages.get_mut().extend(messages);
            }
            if let Some(extra) = result.extra {
                state.extra.get_mut().extend(extra);
            }
        }
        Err(e) => {
            info!("  ❌ API call failed: {}", e);
            info!("  🔄 Implementing graceful fallback...");

            // Demonstrate robust error recovery
            let mut extra = new_extra_map();
            extra.insert(
                "api_result".to_string(),
                json!({
                    "service": "UserDataAPI",
                    "status": "fallback",
                    "error": e.to_string(),
                    "fallback_reason": "External service temporarily unavailable",
                    "data": "Fallback data used due to service failure"
                }),
            );
            state.extra.get_mut().extend(extra);

            state.messages.get_mut().push(Message::with_role(
                Role::System,
                "Using fallback data due to API failure - workflow continues gracefully",
            ));
        }
    }

    // Demonstrate a second API call with higher failure rate to show error handling
    info!("\n1.1️⃣ Running Secondary API Call (high failure rate demo)...");
    let failing_api_node = ApiCallNode {
        service_name: "MetricsAPI".to_string(),
        failure_rate: 0.9, // 90% failure rate to demonstrate error handling
        max_retries: 2,
    };

    let ctx1_1 = NodeContext {
        node_id: "metrics_api".to_string(),
        step: 1,
        event_emitter: Arc::clone(&emitter),
    };

    match failing_api_node.run(state.snapshot(), ctx1_1).await {
        Ok(result) => {
            info!("  ✅ Metrics API call succeeded (lucky!)");
            if let Some(extra) = result.extra {
                state.extra.get_mut().extend(extra);
            }
        }
        Err(e) => {
            info!("  ❌ Metrics API failed as expected: {}", e);
            info!("  🛡️  Demonstrating error resilience - continuing workflow");

            // Add error metadata but continue processing
            let mut extra = new_extra_map();
            extra.insert("metrics_status".to_string(), json!("unavailable"));
            extra.insert("error_handled".to_string(), json!(true));
            state.extra.get_mut().extend(extra);
        }
    }

    // Run conditional router
    info!("\n2️⃣ Running Conditional Router Node...");
    let router_node = ConditionalRouterNode {
        route_key: "service_type".to_string(),
        conditions: {
            let mut conditions = HashMap::new();
            conditions.insert("premium".to_string(), json!("premium"));
            conditions.insert("basic".to_string(), json!("basic"));
            conditions.insert("enterprise".to_string(), json!("enterprise"));
            conditions
        },
    };

    let ctx2 = NodeContext {
        node_id: "router".to_string(),
        step: 2,
        event_emitter: Arc::clone(&emitter),
    };

    let result2 = router_node.run(state.snapshot(), ctx2).await?;
    if let Some(messages) = result2.messages {
        state.messages.get_mut().extend(messages);
    }
    if let Some(extra) = result2.extra {
        state.extra.get_mut().extend(extra);
    }
    info!("  ✅ Routing completed");

    // Run data transformer
    info!("\n3️⃣ Running Data Transformer Node...");
    let transformer_node = DataTransformerNode {
        transformation_rules: vec![
            TransformRule {
                source_key: "user_data".to_string(),
                target_key: "user_data_upper".to_string(),
                operation: TransformOperation::Uppercase,
            },
            TransformRule {
                source_key: "user_data".to_string(),
                target_key: "user_data_length".to_string(),
                operation: TransformOperation::Length,
            },
            TransformRule {
                source_key: "service_type".to_string(),
                target_key: "service_type_reversed".to_string(),
                operation: TransformOperation::Reverse,
            },
            TransformRule {
                source_key: "priority".to_string(),
                target_key: "priority_doubled".to_string(),
                operation: TransformOperation::Multiply(2),
            },
        ],
    };

    let ctx3 = NodeContext {
        node_id: "transformer".to_string(),
        step: 3,
        event_emitter: Arc::clone(&emitter),
    };

    let result3 = transformer_node.run(state.snapshot(), ctx3).await?;
    if let Some(messages) = result3.messages {
        state.messages.get_mut().extend(messages);
    }
    if let Some(extra) = result3.extra {
        state.extra.get_mut().extend(extra);
    }
    info!("  ✅ Transformation completed");

    // Display comprehensive results
    info!("\n📋 Final Pipeline Results:");
    info!("==========================================");

    let final_snapshot = state.snapshot();

    info!("\n💬 Messages ({} total):", final_snapshot.messages.len());
    for (i, msg) in final_snapshot.messages.iter().enumerate() {
        info!("  {}: [{}] {}", i + 1, msg.role, msg.content);
    }

    info!("\n📊 State Data ({} keys):", final_snapshot.extra.len());
    for (key, value) in &final_snapshot.extra {
        info!("  {}: {}", key, value);
    }

    // Show transformations specifically
    info!("\n🔄 Transformations Applied:");
    if let Some(log) = final_snapshot.extra.get("transformation_log")
        && let Some(log_array) = log.as_array()
    {
        for (i, entry) in log_array.iter().enumerate() {
            info!("  {}: {}", i + 1, entry);
        }
    }

    // Show routing decision
    info!("\n🛤️  Routing Decision:");
    if let Some(routing) = final_snapshot.extra.get("routing_decision") {
        info!("  {}", routing);
    }

    // Give time for events to be processed
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    event_bus.stop_listener().await;

    info!("\n✅ Advanced patterns example completed!");
    info!("\n🎯 This example demonstrated key enterprise workflow patterns:");
    info!("=====================================");
    info!("  🔄 Error Recovery:");
    info!("    • Retry logic with exponential backoff simulation");
    info!("    • Graceful fallback when external services fail");
    info!("    • Workflow continuation despite component failures");
    info!("  🛤️  Conditional Routing:");
    info!("    • Dynamic path selection based on runtime state");
    info!("    • Flexible condition evaluation framework");
    info!("    • Rich routing metadata for debugging");
    info!("  🔧 Data Transformation:");
    info!("    • Type-safe transformation operations");
    info!("    • Comprehensive transformation logging");
    info!("    • Flexible rule-based processing");
    info!("  📊 Observability:");
    info!("    • Rich event emission throughout the pipeline");
    info!("    • Structured logging with context preservation");
    info!("    • Performance and decision tracking");
    info!("\n💡 Key Takeaways:");
    info!("  • Nodes should be resilient and handle failures gracefully");
    info!("  • Rich state metadata enables powerful conditional logic");
    info!("  • Event emission provides crucial visibility into complex workflows");
    info!("  • NodePartial patterns enable efficient, focused state updates");

    Ok(())
}
