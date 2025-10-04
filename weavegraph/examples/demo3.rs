//! Demo 3: LLM Integration with Modern Runtime Configuration
//!
//! This demonstration showcases integration of Large Language Models (LLMs) with
//! the Weavegraph workflow framework, featuring modern runtime configuration patterns,
//! persistent checkpointing, and conditional execution flows. This demo builds on
//! the previous examples while introducing real-world AI agent patterns.
//!
//! What You'll Learn:
//! 1. LLM Integration: Using external AI models within workflow nodes
//! 2. Runtime Configuration: Modern config patterns with checkpointing
//! 3. Conditional Edges: Dynamic workflow routing based on state
//! 4. State Persistence: SQLite-backed checkpoint persistence
//! 5. Iterative Processing: Multi-pass content enhancement workflows
//! 6. Error Handling: Robust error management for external service calls
//!
//! Prerequisites:
//! - Ollama installed and running locally
//! - Model 'gemma3:270m' available (or modify to use your preferred model)
//!
//! Running This Demo:
//! ```bash
//! # Start Ollama (in separate terminal)
//! ollama serve
//! ollama pull gemma3:270m  # If not already available
//!
//! # Run the demo
//! cargo run --example demo3
//! ```

use async_trait::async_trait;
use miette::Result;
use rig::client::CompletionClient;
use rig::completion::CompletionModel;
use rig::providers::ollama;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::sync::Arc;
use tracing::instrument;
use weavegraph::channels::{errors::pretty_print, Channel};
use weavegraph::graph::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{CheckpointerType, RuntimeConfig};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

/// First-stage LLM node that generates initial content based on user input.
///
/// This node demonstrates:
/// - External LLM service integration
/// - Error handling for service calls
/// - Modern message construction patterns
/// - Event emission for progress tracking
#[derive(Clone)]
struct ContentGeneratorNode;

#[async_trait]
impl Node for ContentGeneratorNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("llm_start", "Starting content generation with Ollama")?;

        // Extract user input (should be the first/last user message)
        let user_prompt =
            snapshot
                .messages
                .iter()
                .find(|msg| msg.is_user())
                .ok_or(NodeError::MissingInput {
                    what: "user_prompt",
                })?;

        ctx.emit("llm_input", format!("User prompt: {}", user_prompt.content))?;

        // Initialize Ollama client with modern patterns
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3:270m");

        // Build completion request with structured prompt
        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(user_prompt.content.clone()))
            .preamble("You are a creative writing AI assistant. Create engaging, well-structured content based on the user's request. Focus on clarity and creativity.".to_owned())
            .temperature(0.7)
            .build();

        ctx.emit("llm_call", "Calling Ollama API for content generation")?;

        // Make the API call with proper error handling
        let response = completion_model
            .completion(completion_request)
            .await
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: format!("Content generation failed: {}", e),
            })?;

        ctx.emit(
            "llm_response",
            format!("Generated {} response choices", response.choice.len()),
        )?;

        // ✅ MODERN: Use convenience constructor for response message
        let response_content = response
            .choice
            .into_iter()
            .map(|choice| format!("{:?}", choice))
            .collect::<Vec<_>>()
            .join(" ");

        let assistant_message = Message::assistant(&response_content);

        // Add metadata about the generation
        let mut extra_data = FxHashMap::default();
        extra_data.insert("generation_stage".into(), json!("initial"));
        extra_data.insert("model_used".into(), json!("gemma3:270m"));
        extra_data.insert("temperature".into(), json!(0.7));
        extra_data.insert("content_length".into(), json!(response_content.len()));

        ctx.emit("llm_complete", "Content generation completed successfully")?;

        Ok(NodePartial {
            messages: Some(vec![assistant_message]),
            extra: Some(extra_data),
            errors: None,
        })
    }
}

/// Enhancement node that iteratively improves content quality.
///
/// This node demonstrates:
/// - Content enhancement and iteration patterns
/// - State-dependent processing (iteration counting)
/// - Conditional workflow continuation logic
/// - Rich metadata tracking
#[derive(Clone)]
struct ContentEnhancerNode;

#[async_trait]
impl Node for ContentEnhancerNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        // Get current iteration count
        let current_iterations = serde_json::from_value::<i32>(
            snapshot
                .extra
                .get("enhancement_iterations")
                .unwrap_or(&json!(0))
                .clone(),
        )
        .map_err(NodeError::from)?;

        ctx.emit(
            "enhance_start",
            format!(
                "Starting enhancement iteration {} with {} messages",
                current_iterations + 1,
                snapshot.messages.len()
            ),
        )?;

        // Get the most recent assistant message to enhance
        let previous_content = snapshot
            .messages
            .iter()
            .filter(|msg| msg.is_assistant())
            .next_back()
            .ok_or(NodeError::MissingInput {
                what: "previous_assistant_content",
            })?;

        ctx.emit(
            "enhance_input",
            format!(
                "Enhancing content (length: {} chars)",
                previous_content.content.len()
            ),
        )?;

        // Initialize LLM client
        let client = ollama::Client::new();
        let completion_model = client.completion_model("gemma3");

        // Create enhancement prompt based on iteration
        let enhancement_prompt = format!(
            "Enhance and improve this content by adding more detail, better structure, and engaging elements. Current content:\n\n{}\n\nProvide an improved version that is more comprehensive and engaging.",
            previous_content.content
        );

        let completion_request = completion_model
            .completion_request(rig::completion::Message::user(enhancement_prompt))
            .preamble("You are an expert content editor. Improve the given content by adding depth, clarity, and engagement while maintaining the original intent.".to_owned())
            .temperature(0.6)
            .build();

        ctx.emit("enhance_call", "Calling Ollama API for content enhancement")?;

        let response = completion_model
            .completion(completion_request)
            .await
            .map_err(|e| NodeError::Provider {
                provider: "ollama",
                message: format!("Content enhancement failed: {}", e),
            })?;

        // ✅ MODERN: Use convenience constructor
        let enhanced_content = response
            .choice
            .into_iter()
            .map(|choice| format!("{:?}", choice))
            .collect::<Vec<_>>()
            .join(" ");

        let enhanced_message = Message::assistant(&enhanced_content);

        // Update iteration tracking and metadata
        let mut extra_data = FxHashMap::default();
        extra_data.insert(
            "enhancement_iterations".into(),
            json!(current_iterations + 1),
        );
        extra_data.insert("enhancement_stage".into(), json!("enhanced"));
        extra_data.insert("model_used".into(), json!("gemma3"));
        extra_data.insert("temperature".into(), json!(0.6));
        extra_data.insert(
            "enhanced_content_length".into(),
            json!(enhanced_content.len()),
        );
        extra_data.insert(
            "original_content_length".into(),
            json!(previous_content.content.len()),
        );
        extra_data.insert(
            "content_growth_ratio".into(),
            json!(enhanced_content.len() as f64 / previous_content.content.len() as f64),
        );

        ctx.emit(
            "enhance_complete",
            format!(
                "Enhancement completed. Iteration: {}, Growth: {:.1}x",
                current_iterations + 1,
                enhanced_content.len() as f64 / previous_content.content.len() as f64
            ),
        )?;

        Ok(NodePartial {
            messages: Some(vec![enhanced_message]),
            extra: Some(extra_data),
            errors: None,
        })
    }
}

/// Demonstration of LLM integration with modern runtime configuration patterns.
///
/// This demo showcases:
/// - **External LLM Integration**: Using Ollama for content generation
/// - **Modern Runtime Config**: SQLite checkpointing with session management  
/// - **Conditional Workflows**: Dynamic routing based on iteration count
/// - **State Persistence**: Checkpoint-based resumable execution
/// - **Error Handling**: Robust management of external service calls
/// - **Iterative Processing**: Multi-pass content enhancement
///
/// # Workflow Architecture
///
/// ```text
/// Start → ContentGenerator → ContentEnhancer ⟲ (conditional loop)
///                              ↓ (when done)
///                             End
/// ```
///
/// The workflow uses a conditional edge that loops the ContentEnhancer
/// until a specified number of iterations is reached, demonstrating
/// sophisticated control flow patterns.
///
/// # Expected Behavior
///
/// 1. Generate initial content from user prompt
/// 2. Enhance content iteratively (configurable iterations)
/// 3. Track progress with rich metadata
/// 4. Persist state at each step (SQLite checkpointing)
/// 5. Demonstrate resumable execution patterns
#[tokio::main]
async fn main() -> Result<()> {
    demo().await
}

#[instrument]
async fn demo() -> Result<()> {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                        Demo 3                           ║");
    println!("║         LLM Integration & Runtime Configuration         ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // ✅ STEP 1: Modern State Construction with LLM Context
    println!("📊 Step 1: Creating initial state with LLM workflow context");

    let init = VersionedState::builder()
        .with_user_message("Write a comprehensive guide about sustainable gardening practices")
        .with_extra("workflow_type", json!("llm_content_generation"))
        .with_extra("target_iterations", json!(2))
        .with_extra("enhancement_iterations", json!(0))
        .with_extra(
            "model_config",
            json!({
                "generator_model": "gemma3:270m",
                "enhancer_model": "gemma3",
                "generator_temperature": 0.7,
                "enhancer_temperature": 0.6
            }),
        )
        .with_extra(
            "quality_metrics",
            json!({
                "track_content_growth": true,
                "track_iteration_timing": true,
                "track_model_performance": true
            }),
        )
        .build();

    println!("   ✓ LLM workflow state initialized");
    println!("   ✓ User topic: {}", init.messages.snapshot()[0].content);
    println!(
        "   ✓ Target iterations: {}",
        init.extra.snapshot()["target_iterations"]
    );

    // ✅ STEP 2: Modern Runtime Configuration with Persistence
    println!("\n⚙️  Step 2: Configuring runtime with SQLite checkpointing");

    let runtime_config = RuntimeConfig::new(
        Some("llm_demo_session".to_string()),
        Some(CheckpointerType::SQLite),
        Some("weavegraph_demo3.db".to_string()),
    );

    println!("   ✓ Runtime configured with persistent checkpointing");
    println!("   ✓ Session ID: {:?}", runtime_config.session_id);
    println!("   ✓ Checkpointer: {:?}", runtime_config.checkpointer);
    println!("   ✓ Database: {:?}", runtime_config.sqlite_db_name);

    // ✅ STEP 3: Building Graph with Conditional Logic
    println!("\n🔗 Step 3: Building LLM workflow with conditional enhancement loop");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Other("ContentGenerator".into()),
            ContentGeneratorNode,
        )
        .add_node(
            NodeKind::Other("ContentEnhancer".into()),
            ContentEnhancerNode,
        )
        // Start flows to content generator
        .add_edge(NodeKind::Start, NodeKind::Other("ContentGenerator".into()))
        // Generator flows to enhancer
        .add_edge(
            NodeKind::Other("ContentGenerator".into()),
            NodeKind::Other("ContentEnhancer".into()),
        )
        // Conditional edge: enhancer loops back to itself or goes to end
        .add_conditional_edge(
            NodeKind::Other("ContentEnhancer".into()),
            NodeKind::End,
            NodeKind::Other("ContentEnhancer".into()),
            Arc::new(|snapshot: StateSnapshot| {
                // Continue enhancing if we haven't reached target iterations
                let current_iterations = serde_json::from_value::<i32>(
                    snapshot
                        .extra
                        .get("enhancement_iterations")
                        .unwrap_or(&json!(0))
                        .clone(),
                )
                .unwrap_or(0);

                let target_iterations = serde_json::from_value::<i32>(
                    snapshot
                        .extra
                        .get("target_iterations")
                        .unwrap_or(&json!(2))
                        .clone(),
                )
                .unwrap_or(2);

                // Return true to continue to End, false to loop back to enhancer
                current_iterations >= target_iterations
            }),
        )
        .set_entry(NodeKind::Start)
        .with_runtime_config(runtime_config)
        .compile()
        .map_err(|e| miette::miette!("Graph compilation failed: {e:?}"))?;

    println!("   ✓ LLM workflow graph compiled successfully");
    println!("   ✓ Nodes: ContentGenerator → ContentEnhancer (conditional loop) → End");
    println!("   ✓ Conditional logic: Loop until target iterations reached");
    println!("   ✓ Persistent checkpointing enabled");

    // ✅ STEP 4: Execute LLM Workflow with Monitoring
    println!("\n🚀 Step 4: Executing LLM workflow with real-time monitoring");
    println!("   📡 Note: This will make actual calls to Ollama - ensure it's running!");

    let execution_start = std::time::Instant::now();

    let final_state = app
        .invoke(init)
        .await
        .map_err(|e| miette::miette!("LLM workflow execution failed: {e}"))?;

    let execution_duration = execution_start.elapsed();

    println!("   ✅ LLM workflow completed successfully");
    println!(
        "   ⏱️  Total execution time: {:.2}s",
        execution_duration.as_secs_f64()
    );

    // ✅ STEP 5: Analyze Results and Content Evolution
    println!("\n📊 Step 5: Analyzing content generation results");

    let final_snapshot = final_state.snapshot();

    println!("   📈 Workflow Statistics:");
    println!("      • Total messages: {}", final_snapshot.messages.len());
    println!(
        "      • Messages version: {}",
        final_snapshot.messages_version
    );
    println!("      • Extra data entries: {}", final_snapshot.extra.len());
    println!(
        "      • Final iterations: {}",
        final_snapshot
            .extra
            .get("enhancement_iterations")
            .unwrap_or(&json!(0))
    );

    // Display content evolution
    println!("\n   📝 Content Evolution Timeline:");
    let user_messages: Vec<_> = final_snapshot
        .messages
        .iter()
        .filter(|msg| msg.is_user())
        .collect();
    let assistant_messages: Vec<_> = final_snapshot
        .messages
        .iter()
        .filter(|msg| msg.is_assistant())
        .collect();

    println!("      1. User Request:");
    if let Some(user_msg) = user_messages.first() {
        println!("         \"{}\"", user_msg.content);
    }

    for (i, msg) in assistant_messages.iter().enumerate() {
        println!("      {}. Assistant Response (iteration {}):", i + 2, i + 1);
        let preview = if msg.content.len() > 100 {
            format!("{}...", &msg.content[..100])
        } else {
            msg.content.clone()
        };
        println!("         \"{}\"", preview);
        println!("         Length: {} characters", msg.content.len());
    }

    // Display performance metrics
    if let Some(metrics) = final_snapshot.extra.get("content_growth_ratio") {
        println!("\n   📊 Content Quality Metrics:");
        println!(
            "      • Content growth ratio: {:.2}x",
            metrics.as_f64().unwrap_or(1.0)
        );
    }

    // Show model usage statistics
    println!("\n   🤖 Model Usage:");
    let generator_calls = assistant_messages.len().min(1);
    let enhancer_calls = assistant_messages.len().saturating_sub(1);
    println!("      • Generator calls (gemma3:270m): {}", generator_calls);
    println!("      • Enhancer calls (gemma3): {}", enhancer_calls);
    println!(
        "      • Total LLM calls: {}",
        generator_calls + enhancer_calls
    );

    // ✅ STEP 6: Persistence and Checkpoint Analysis
    println!("\n💾 Step 6: Checkpoint persistence analysis");

    println!(
        "   ✓ State persisted to SQLite database: {:?}",
        app.runtime_config()
            .sqlite_db_name
            .as_ref()
            .unwrap_or(&"weavegraph.db".to_string())
    );
    println!(
        "   ✓ Session ID: {:?}",
        app.runtime_config()
            .session_id
            .as_ref()
            .unwrap_or(&"default".to_string())
    );
    println!("   ✓ Workflow resumable from any checkpoint");

    // Check for errors
    let errors = final_state.errors.snapshot();
    if !errors.is_empty() {
        println!("\n   ⚠️  Errors captured during execution:");
        println!("{}", pretty_print(&errors));
    } else {
        println!("\n   ✅ No errors encountered during LLM workflow");
    }

    // ✅ STEP 7: Final Content Display
    println!("\n📋 Step 7: Final enhanced content");

    if let Some(final_content) = assistant_messages.last() {
        println!("   ╭─────────────────────────────────────────────────────────╮");
        println!("   │                    FINAL CONTENT                        │");
        println!("   ╰─────────────────────────────────────────────────────────╯");

        // Display content in chunks for readability
        let content = &final_content.content;
        let chunk_size = 80;
        for chunk in content.chars().collect::<Vec<_>>().chunks(chunk_size) {
            let line: String = chunk.iter().collect();
            println!("   {}", line);
        }

        println!("\n   📏 Final content statistics:");
        println!("      • Character count: {}", content.len());
        println!(
            "      • Estimated word count: {}",
            content.split_whitespace().count()
        );
        println!("      • Line count: {}", content.lines().count());
    }

    // ✅ FINAL SUMMARY
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                      Demo 3 Complete                    ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("\n✅ LLM integration patterns demonstrated:");
    println!("   • External LLM service integration (Ollama)");
    println!("   • Modern runtime configuration with persistence");
    println!("   • Conditional workflow routing and loops");
    println!("   • Iterative content enhancement workflows");
    println!("   • SQLite-backed checkpoint persistence");
    println!("   • Robust error handling for external services");
    println!("   • Rich metadata tracking and performance analysis");
    println!("\n🎯 Next: Run demo4 to see streaming LLM integration");
    println!("💡 Tip: Check the SQLite database for persisted checkpoints!");

    Ok(())
}
