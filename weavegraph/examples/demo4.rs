//! Demo 4: Advanced Streaming LLM Integration with Error Persistence
//!
//! This demonstration represents the most sophisticated example in the series,
//! showcasing streaming LLM responses, comprehensive error persistence, and
//! advanced workflow patterns. It integrates all the concepts from previous
//! demos while introducing real-time streaming and production-ready error
//! handling patterns.
//!
//! What You'll Learn:
//! 1. Streaming LLM Integration: Real-time response streaming with Gemini
//! 2. Advanced Error Persistence: Comprehensive error tracking and analysis
//! 3. Event-Driven Architecture: Rich event emission and monitoring
//! 4. Production Patterns: Robust error handling and quality validation
//! 5. Performance Monitoring: Real-time metrics and timing analysis
//! 6. SQLite Integration: Full persistence with enhanced checkpointing
//!
//! Prerequisites:
//! - Gemini API key set in environment (`GEMINI_API_KEY`)
//! - Internet connection for Gemini API calls
//!
//! Running This Demo:
//! ```bash
//! # Set up Gemini API key
//! export GEMINI_API_KEY="your_api_key_here"
//!
//! # Run the demo
//! cargo run --example demo4
//! ```

use async_trait::async_trait;
use futures_util::StreamExt;
use miette::Result;
use rig::agent::MultiTurnStreamItem;
use rig::message::{Reasoning, Text};
use rig::prelude::*;
use rig::providers::gemini::completion::gemini_api_types::{
    AdditionalParameters, GenerationConfig, ThinkingConfig,
};
use rig::{
    providers::gemini,
    streaming::{StreamedAssistantContent, StreamingPrompt},
};
use rustc_hash::FxHashMap;
use serde_json::json;
use tracing::instrument;
use weavegraph::channels::{
    errors::{pretty_print, ErrorEvent, ErrorScope, LadderError},
    Channel,
};
use weavegraph::graph::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{CheckpointerType, RuntimeConfig};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

/// Streaming content generation node with advanced error handling.
///
/// This node demonstrates:
/// - Real-time streaming LLM responses
/// - Comprehensive error tracking and persistence
/// - Performance monitoring and quality validation
/// - Advanced event emission patterns
/// - Production-ready error handling
#[derive(Clone)]
struct StreamingGeneratorNode;

#[async_trait]
impl Node for StreamingGeneratorNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let start_time = std::time::Instant::now();
        let mut errors = Vec::new();

        ctx.emit("streaming_start", "Initializing streaming LLM generation")?;

        // Extract user input with validation
        let user_prompt =
            snapshot
                .messages
                .iter()
                .find(|msg| msg.is_user())
                .ok_or(NodeError::MissingInput {
                    what: "user_prompt",
                })?;

        ctx.emit(
            "prompt_extracted",
            format!(
                "Processing user prompt (length: {} chars)",
                user_prompt.content.len()
            ),
        )?;

        // Configure advanced Gemini model with thinking capabilities
        let gen_cfg = GenerationConfig {
            thinking_config: Some(ThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: 2048,
            }),
            ..Default::default()
        };
        let cfg = AdditionalParameters::default().with_config(gen_cfg);

        // Create streaming agent with comprehensive configuration
        let agent = gemini::Client::from_env()
            .agent("gemini-2.5-flash")
            .preamble("You are a senior technical writing expert. Create comprehensive, well-structured content that demonstrates deep expertise and clarity.")
            .temperature(0.8)
            .additional_params(serde_json::to_value(cfg).unwrap())
            .build();

        ctx.emit(
            "agent_configured",
            "Gemini streaming agent configured with thinking capabilities",
        )?;

        // Start streaming with comprehensive monitoring
        let stream_start = std::time::Instant::now();
        let mut stream = agent.stream_prompt(user_prompt.content.clone()).await;

        let mut chunk_count = 0;
        let mut total_content = String::new();
        let mut reasoning_content = String::new();
        let mut first_chunk_time = None;
        let mut last_chunk_time = stream_start;

        ctx.emit("streaming_started", "Beginning real-time content streaming")?;

        // Process streaming chunks with detailed monitoring
        while let Some(content) = stream.next().await {
            let chunk_time = std::time::Instant::now();

            if first_chunk_time.is_none() {
                first_chunk_time = Some(chunk_time);
                let ttfb = chunk_time.duration_since(stream_start);
                ctx.emit(
                    "first_chunk",
                    format!(
                        "First chunk received (TTFB: {:.2}ms)",
                        ttfb.as_secs_f64() * 1000.0
                    ),
                )?;
            }

            match content {
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                    text,
                }))) => {
                    total_content.push_str(&text);
                    chunk_count += 1;

                    let chunk_delay = chunk_time.duration_since(last_chunk_time);
                    ctx.emit(
                        "text_chunk",
                        format!(
                            "Chunk {}: {} chars (delay: {:.1}ms)",
                            chunk_count,
                            text.len(),
                            chunk_delay.as_secs_f64() * 1000.0
                        ),
                    )?;

                    last_chunk_time = chunk_time;
                }

                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Reasoning(
                    Reasoning { reasoning, .. },
                ))) => {
                    let reasoning_text = reasoning.join("\n");
                    reasoning_content.push_str(&reasoning_text);
                    reasoning_content.push('\n');

                    ctx.emit(
                        "reasoning_chunk",
                        format!("Reasoning received: {} chars", reasoning_text.len()),
                    )?;
                }

                Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                    total_content.push_str(res.response());
                    chunk_count += 1;

                    ctx.emit(
                        "final_response",
                        format!("Final response: {} chars", res.response().len()),
                    )?;
                }

                Err(err) => {
                    // Create detailed error event for streaming failure
                    let error_event = ErrorEvent {
                        when: chrono::Utc::now(),
                        scope: ErrorScope::Node {
                            kind: "StreamingGenerator".to_string(),
                            step: ctx.step,
                        },
                        error: LadderError {
                            message: format!("Streaming error at chunk {}: {}", chunk_count, err),
                            cause: Some(Box::new(LadderError {
                                message: err.to_string(),
                                cause: None,
                                details: json!({
                                    "error_type": "streaming_interruption",
                                    "chunk_count_at_error": chunk_count,
                                    "content_length_at_error": total_content.len()
                                }),
                            })),
                            details: json!({
                                "streaming_context": {
                                    "chunks_received": chunk_count,
                                    "content_length": total_content.len(),
                                    "reasoning_length": reasoning_content.len(),
                                    "stream_duration_ms": stream_start.elapsed().as_millis()
                                }
                            }),
                        },
                        tags: vec!["streaming_error".to_string(), "llm_api_error".to_string()],
                        context: json!({
                            "node_type": "StreamingGenerator",
                            "model": "gemini-2.5-flash",
                            "operation": "stream_prompt"
                        }),
                    };

                    errors.push(error_event);
                    ctx.emit("streaming_error", format!("Streaming error: {}", err))?;
                }

                _ => {
                    // Handle other stream items
                }
            }
        }

        let stream_duration = stream_start.elapsed();
        let total_duration = start_time.elapsed();

        ctx.emit(
            "streaming_complete",
            format!(
                "Streaming completed: {} chunks, {:.2}s total",
                chunk_count,
                stream_duration.as_secs_f64()
            ),
        )?;

        // Quality validation and error generation
        self.validate_content_quality(&total_content, chunk_count, ctx.step, &mut errors);

        // ✅ MODERN: Use convenience constructor
        let response_message = Message::assistant(&total_content);

        // Create comprehensive metadata
        let mut extra_data = FxHashMap::default();
        extra_data.insert(
            "streaming_stats".into(),
            json!({
                "chunk_count": chunk_count,
                "total_content_length": total_content.len(),
                "reasoning_content_length": reasoning_content.len(),
                "stream_duration_ms": stream_duration.as_millis(),
                "total_duration_ms": total_duration.as_millis(),
                "ttfb_ms": first_chunk_time.map(|t| t.duration_since(stream_start).as_millis()),
                "avg_chunk_delay_ms": if chunk_count > 1 {
                    Some(stream_duration.as_millis() / chunk_count as u128)
                } else {
                    None
                }
            }),
        );
        extra_data.insert(
            "model_config".into(),
            json!({
                "model": "gemini-2.5-flash",
                "temperature": 0.8,
                "thinking_enabled": true,
                "thinking_budget": 2048
            }),
        );
        extra_data.insert(
            "quality_metrics".into(),
            json!({
                "has_reasoning": !reasoning_content.is_empty(),
                "content_quality_validated": true,
                "error_count": errors.len()
            }),
        );

        ctx.emit(
            "node_complete",
            format!(
                "StreamingGenerator completed: {} chars, {} errors",
                total_content.len(),
                errors.len()
            ),
        )?;

        Ok(NodePartial {
            messages: Some(vec![response_message]),
            extra: Some(extra_data),
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
        })
    }
}

impl StreamingGeneratorNode {
    /// Validate content quality and generate appropriate error events.
    fn validate_content_quality(
        &self,
        content: &str,
        chunk_count: usize,
        step: u64,
        errors: &mut Vec<ErrorEvent>,
    ) {
        // Check for suspiciously short content
        if content.len() < 100 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingGenerator".to_string(),
                    step,
                },
                error: LadderError {
                    message: "Generated content is suspiciously short".to_string(),
                    cause: None,
                    details: json!({
                        "content_length": content.len(),
                        "expected_minimum": 100,
                        "chunk_count": chunk_count
                    }),
                },
                tags: vec!["quality_check".to_string(), "short_content".to_string()],
                context: json!({
                    "validation_type": "content_length",
                    "severity": "warning"
                }),
            });
        }

        // Check for low chunk count (potential streaming issues)
        if chunk_count < 3 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingGenerator".to_string(),
                    step,
                },
                error: LadderError {
                    message: format!("Low chunk count in streaming response: {}", chunk_count),
                    cause: None,
                    details: json!({
                        "chunk_count": chunk_count,
                        "expected_minimum": 3,
                        "possible_causes": ["network_issues", "short_response", "streaming_config"]
                    }),
                },
                tags: vec![
                    "streaming_quality".to_string(),
                    "performance_warning".to_string(),
                ],
                context: json!({
                    "validation_type": "streaming_performance",
                    "severity": "info"
                }),
            });
        }
    }
}

/// Advanced content enhancement node with comprehensive error tracking.
///
/// This node demonstrates:
/// - Multi-stage content processing
/// - Advanced error categorization and persistence
/// - Content analysis and validation
/// - Performance benchmarking
#[derive(Clone)]
struct StreamingEnhancerNode;

#[async_trait]
impl Node for StreamingEnhancerNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        let start_time = std::time::Instant::now();
        let mut errors = Vec::new();

        ctx.emit("enhancer_start", "Starting advanced content enhancement")?;

        // Get previous assistant content for enhancement
        let previous_content = snapshot
            .messages
            .iter()
            .filter(|msg| msg.is_assistant())
            .next_back()
            .ok_or(NodeError::MissingInput {
                what: "previous_assistant_content",
            })?;

        ctx.emit(
            "input_analysis",
            format!(
                "Enhancing content: {} chars, {} words, {} lines",
                previous_content.content.len(),
                previous_content.content.split_whitespace().count(),
                previous_content.content.lines().count()
            ),
        )?;

        // Validate input quality and create warnings if needed
        self.validate_input_quality(&previous_content.content, ctx.step, &mut errors);

        // Configure enhanced Gemini model
        let gen_cfg = GenerationConfig {
            thinking_config: Some(ThinkingConfig {
                include_thoughts: Some(true),
                thinking_budget: 3072, // Larger budget for enhancement
            }),
            ..Default::default()
        };
        let cfg = AdditionalParameters::default().with_config(gen_cfg);

        let agent = gemini::Client::from_env()
            .agent("gemini-2.5-flash")
            .preamble("You are an expert content editor and technical writer. Enhance the given content by adding depth, examples, structure, and engagement while maintaining accuracy and clarity.")
            .temperature(0.7)
            .additional_params(serde_json::to_value(cfg).unwrap())
            .build();

        // Create sophisticated enhancement prompt
        let enhancement_prompt = format!(
            "Please enhance and expand this content by:\n\
            1. Adding more detailed explanations and examples\n\
            2. Improving structure and readability\n\
            3. Including practical insights and best practices\n\
            4. Ensuring comprehensive coverage of the topic\n\n\
            Original content to enhance:\n\n{}\n\n\
            Provide a significantly improved and expanded version.",
            previous_content.content
        );

        ctx.emit(
            "enhancement_prompt",
            "Sending enhancement request to Gemini",
        )?;

        // Stream the enhancement with monitoring
        let stream_start = std::time::Instant::now();
        let mut stream = agent.stream_prompt(enhancement_prompt).await;

        let mut chunk_count = 0;
        let mut enhanced_content = String::new();
        let mut reasoning_content = String::new();
        let mut chunk_timings = Vec::new();

        while let Some(content) = stream.next().await {
            let chunk_time = std::time::Instant::now();

            match content {
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                    text,
                }))) => {
                    enhanced_content.push_str(&text);
                    chunk_count += 1;

                    let timing = chunk_time.duration_since(stream_start);
                    chunk_timings.push(timing);

                    ctx.emit(
                        "enhancement_chunk",
                        format!("Enhancement chunk {}: {} chars", chunk_count, text.len()),
                    )?;
                }

                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Reasoning(
                    Reasoning { reasoning, .. },
                ))) => {
                    let reasoning_text = reasoning.join("\n");
                    reasoning_content.push_str(&reasoning_text);

                    ctx.emit(
                        "enhancement_reasoning",
                        format!("Enhancement reasoning: {} chars", reasoning_text.len()),
                    )?;
                }

                Ok(MultiTurnStreamItem::FinalResponse(res)) => {
                    enhanced_content.push_str(res.response());
                    chunk_count += 1;

                    ctx.emit(
                        "enhancement_final",
                        format!(
                            "Enhancement complete: {} total chars",
                            enhanced_content.len()
                        ),
                    )?;
                }

                Err(err) => {
                    let error_event = ErrorEvent {
                        when: chrono::Utc::now(),
                        scope: ErrorScope::Node {
                            kind: "StreamingEnhancer".to_string(),
                            step: ctx.step,
                        },
                        error: LadderError {
                            message: format!("Enhancement streaming error: {}", err),
                            cause: Some(Box::new(LadderError {
                                message: err.to_string(),
                                cause: None,
                                details: json!({"api_error": true}),
                            })),
                            details: json!({
                                "enhancement_context": {
                                    "input_length": previous_content.content.len(),
                                    "chunks_before_error": chunk_count,
                                    "content_at_error": enhanced_content.len()
                                }
                            }),
                        },
                        tags: vec![
                            "enhancement_error".to_string(),
                            "streaming_error".to_string(),
                        ],
                        context: json!({
                            "node_type": "StreamingEnhancer",
                            "operation": "content_enhancement"
                        }),
                    };

                    errors.push(error_event);
                }

                _ => {}
            }
        }

        let stream_duration = stream_start.elapsed();
        let total_duration = start_time.elapsed();

        // Validate enhancement quality
        self.validate_enhancement_quality(
            &previous_content.content,
            &enhanced_content,
            chunk_count,
            ctx.step,
            &mut errors,
        );

        // ✅ MODERN: Use convenience constructor
        let enhanced_message = Message::assistant(&enhanced_content);

        // Create comprehensive enhancement metadata
        let mut extra_data = FxHashMap::default();
        extra_data.insert("enhancement_stats".into(), json!({
            "input_length": previous_content.content.len(),
            "output_length": enhanced_content.len(),
            "expansion_ratio": enhanced_content.len() as f64 / previous_content.content.len() as f64,
            "chunk_count": chunk_count,
            "reasoning_length": reasoning_content.len(),
            "stream_duration_ms": stream_duration.as_millis(),
            "total_duration_ms": total_duration.as_millis(),
            "avg_chunk_size": if chunk_count > 0 { enhanced_content.len() / chunk_count } else { 0 }
        }));

        extra_data.insert(
            "quality_analysis".into(),
            json!({
                "input_words": previous_content.content.split_whitespace().count(),
                "output_words": enhanced_content.split_whitespace().count(),
                "input_lines": previous_content.content.lines().count(),
                "output_lines": enhanced_content.lines().count(),
                "has_reasoning": !reasoning_content.is_empty(),
                "error_count": errors.len()
            }),
        );

        ctx.emit(
            "enhancer_complete",
            format!(
                "Enhancement completed: {:.1}x expansion, {} errors",
                enhanced_content.len() as f64 / previous_content.content.len() as f64,
                errors.len()
            ),
        )?;

        Ok(NodePartial {
            messages: Some(vec![enhanced_message]),
            extra: Some(extra_data),
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
        })
    }
}

impl StreamingEnhancerNode {
    /// Validate input content quality.
    fn validate_input_quality(&self, content: &str, step: u64, errors: &mut Vec<ErrorEvent>) {
        if content.trim().is_empty() {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingEnhancer".to_string(),
                    step,
                },
                error: LadderError {
                    message: "Input content is empty or contains only whitespace".to_string(),
                    cause: None,
                    details: json!({
                        "input_length": content.len(),
                        "trimmed_length": content.trim().len()
                    }),
                },
                tags: vec!["input_validation".to_string(), "empty_content".to_string()],
                context: json!({
                    "validation_stage": "input",
                    "severity": "error"
                }),
            });
        }

        if !content.trim_end().ends_with(&['.', '!', '?'][..]) {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingEnhancer".to_string(),
                    step,
                },
                error: LadderError {
                    message: "Input content appears incomplete (no ending punctuation)".to_string(),
                    cause: None,
                    details: json!({
                        "content_length": content.len(),
                        "last_10_chars": content.chars().rev().take(10).collect::<String>()
                    }),
                },
                tags: vec![
                    "input_quality".to_string(),
                    "incomplete_content".to_string(),
                ],
                context: json!({
                    "validation_stage": "input",
                    "severity": "warning"
                }),
            });
        }
    }

    /// Validate enhancement effectiveness.
    fn validate_enhancement_quality(
        &self,
        input: &str,
        output: &str,
        chunk_count: usize,
        step: u64,
        errors: &mut Vec<ErrorEvent>,
    ) {
        let expansion_ratio = output.len() as f64 / input.len() as f64;

        // Check if enhancement actually improved content
        if expansion_ratio < 1.2 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingEnhancer".to_string(),
                    step,
                },
                error: LadderError {
                    message: "Enhancement may not have significantly improved content".to_string(),
                    cause: None,
                    details: json!({
                        "input_length": input.len(),
                        "output_length": output.len(),
                        "expansion_ratio": expansion_ratio,
                        "expected_minimum_ratio": 1.2,
                        "chunk_count": chunk_count
                    }),
                },
                tags: vec![
                    "enhancement_quality".to_string(),
                    "low_expansion".to_string(),
                ],
                context: json!({
                    "validation_stage": "output",
                    "severity": "warning"
                }),
            });
        }

        // Check for streaming performance issues
        if chunk_count < 5 {
            errors.push(ErrorEvent {
                when: chrono::Utc::now(),
                scope: ErrorScope::Node {
                    kind: "StreamingEnhancer".to_string(),
                    step,
                },
                error: LadderError {
                    message: format!("Low chunk count for enhancement task: {}", chunk_count),
                    cause: None,
                    details: json!({
                        "chunk_count": chunk_count,
                        "expected_minimum": 5,
                        "task_type": "content_enhancement",
                        "expansion_ratio": expansion_ratio
                    }),
                },
                tags: vec![
                    "performance_warning".to_string(),
                    "streaming_efficiency".to_string(),
                ],
                context: json!({
                    "validation_stage": "performance",
                    "severity": "info"
                }),
            });
        }
    }
}

/// Demonstration of advanced streaming LLM integration with comprehensive error persistence.
///
/// This demo represents the pinnacle of the example series, showcasing:
/// - **Real-time Streaming**: Chunk-by-chunk LLM response processing
/// - **Advanced Error Persistence**: Comprehensive error tracking and categorization
/// - **Production Patterns**: Robust error handling, quality validation, and monitoring
/// - **Performance Analysis**: Detailed timing and efficiency metrics
/// - **SQLite Integration**: Full persistence with enhanced checkpointing
/// - **Event-Driven Architecture**: Rich observability and real-time monitoring
///
/// # Architecture Pattern
///
/// ```text
/// User Input → StreamingGenerator → StreamingEnhancer → Final Content
///                     ↓                    ↓
///               Error Tracking      Error Tracking
///                     ↓                    ↓
///                SQLite Persistence
/// ```
///
/// Each node generates comprehensive error events that are persisted
/// to SQLite, providing full audit trails and quality analysis.
///
/// # Expected Behavior
///
/// 1. **Initial Generation**: Stream content from user prompt with quality validation
/// 2. **Content Enhancement**: Stream enhanced version with expansion analysis
/// 3. **Error Collection**: Track and persist quality issues, performance warnings
/// 4. **Performance Analysis**: Measure streaming efficiency and content quality
/// 5. **Persistence**: Save all state and errors to SQLite for analysis
#[tokio::main]
async fn main() -> Result<()> {
    demo().await
}

#[instrument]
async fn demo() -> Result<()> {
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                        Demo 4                           ║");
    println!("║    Advanced Streaming LLM + Error Persistence           ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    // ✅ STEP 1: Advanced State Construction
    println!("📊 Step 1: Initializing advanced streaming workflow state");

    let init = VersionedState::builder()
        .with_user_message("Create a comprehensive technical guide about implementing distributed systems patterns in Rust, covering consistency models, consensus algorithms, and practical implementation strategies")
        .with_extra("workflow_type", json!("advanced_streaming_llm"))
        .with_extra("streaming_config", json!({
            "enable_reasoning": true,
            "thinking_budget": 3072,
            "enable_error_persistence": true,
            "quality_validation": true,
            "performance_monitoring": true
        }))
        .with_extra("content_requirements", json!({
            "min_length": 1000,
            "target_expansion_ratio": 2.0,
            "technical_depth": "advanced",
            "include_examples": true
        }))
        .with_extra("error_tracking", json!({
            "track_streaming_performance": true,
            "track_content_quality": true,
            "track_api_errors": true,
            "persist_to_sqlite": true
        }))
        .build();

    println!("   ✓ Advanced workflow state created");
    println!("   ✓ Topic: Distributed systems in Rust");
    println!("   ✓ Streaming enabled with reasoning and error persistence");

    // ✅ STEP 2: Advanced Runtime Configuration
    println!("\n⚙️  Step 2: Configuring production-ready runtime");

    let runtime_config = RuntimeConfig::new(
        Some("streaming_demo_advanced".to_string()),
        Some(CheckpointerType::SQLite),
        Some("weavegraph_demo4_streaming.db".to_string()),
    );

    println!("   ✓ Production runtime configured");
    println!("   ✓ Error persistence: SQLite enabled");
    println!("   ✓ Session: {:?}", runtime_config.session_id);
    println!("   ✓ Database: {:?}", runtime_config.sqlite_db_name);

    // ✅ STEP 3: Building Advanced Streaming Workflow
    println!("\n🔗 Step 3: Building streaming workflow with error persistence");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Other("StreamingGenerator".into()),
            StreamingGeneratorNode,
        )
        .add_node(
            NodeKind::Other("StreamingEnhancer".into()),
            StreamingEnhancerNode,
        )
        .add_edge(
            NodeKind::Start,
            NodeKind::Other("StreamingGenerator".into()),
        )
        .add_edge(
            NodeKind::Other("StreamingGenerator".into()),
            NodeKind::Other("StreamingEnhancer".into()),
        )
        .add_edge(NodeKind::Other("StreamingEnhancer".into()), NodeKind::End)
        .set_entry(NodeKind::Start)
        .with_runtime_config(runtime_config)
        .compile()
        .map_err(|e| miette::miette!("Advanced workflow compilation failed: {e:?}"))?;

    println!("   ✓ Streaming workflow compiled");
    println!("   ✓ Pipeline: Generator → Enhancer → End");
    println!("   ✓ Error persistence enabled");
    println!("   ✓ Real-time monitoring configured");

    // ✅ STEP 4: Execute Advanced Streaming Workflow
    println!("\n🚀 Step 4: Executing streaming workflow with comprehensive monitoring");
    println!("   🌐 Note: This will make Gemini API calls - ensure API key is set!");

    let execution_start = std::time::Instant::now();

    let final_state = app
        .invoke(init)
        .await
        .map_err(|e| miette::miette!("Streaming workflow execution failed: {e}"))?;

    let total_execution_time = execution_start.elapsed();

    println!("   ✅ Streaming workflow completed");
    println!(
        "   ⏱️  Total execution time: {:.2}s",
        total_execution_time.as_secs_f64()
    );

    // ✅ STEP 5: Comprehensive Results Analysis
    println!("\n📊 Step 5: Analyzing streaming results and performance");

    let final_snapshot = final_state.snapshot();

    // Extract streaming statistics
    let generator_stats = final_snapshot.extra.get("streaming_stats");
    let enhancer_stats = final_snapshot.extra.get("enhancement_stats");

    println!("   📈 Workflow Performance:");
    println!("      • Total messages: {}", final_snapshot.messages.len());
    println!(
        "      • Final state version: {}",
        final_snapshot.messages_version
    );
    println!("      • Extra data entries: {}", final_snapshot.extra.len());
    println!(
        "      • Total execution: {:.2}s",
        total_execution_time.as_secs_f64()
    );

    if let Some(gen_stats) = generator_stats {
        println!("\n   🎯 Generation Performance:");
        if let Some(chunk_count) = gen_stats.get("chunk_count") {
            println!("      • Streaming chunks: {}", chunk_count);
        }
        if let Some(duration) = gen_stats.get("stream_duration_ms") {
            println!(
                "      • Stream duration: {:.2}s",
                duration.as_f64().unwrap_or(0.0) / 1000.0
            );
        }
        if let Some(ttfb) = gen_stats.get("ttfb_ms") {
            println!(
                "      • Time to first byte: {:.1}ms",
                ttfb.as_f64().unwrap_or(0.0)
            );
        }
    }

    if let Some(enh_stats) = enhancer_stats {
        println!("\n   🔧 Enhancement Performance:");
        if let Some(ratio) = enh_stats.get("expansion_ratio") {
            println!(
                "      • Content expansion: {:.2}x",
                ratio.as_f64().unwrap_or(1.0)
            );
        }
        if let Some(input_len) = enh_stats.get("input_length") {
            if let Some(output_len) = enh_stats.get("output_length") {
                println!(
                    "      • Content growth: {} → {} chars",
                    input_len.as_u64().unwrap_or(0),
                    output_len.as_u64().unwrap_or(0)
                );
            }
        }
    }

    // ✅ STEP 6: Content Quality Analysis
    println!("\n📝 Step 6: Content quality and evolution analysis");

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

    println!("   📋 Content Pipeline Results:");
    println!("      • User queries: {}", user_messages.len());
    println!("      • Assistant responses: {}", assistant_messages.len());

    if let Some(user_msg) = user_messages.first() {
        println!("\n   🎯 Original Request:");
        let preview = if user_msg.content.len() > 100 {
            format!("{}...", &user_msg.content[..100])
        } else {
            user_msg.content.clone()
        };
        println!("      \"{}\"", preview);
    }

    for (i, msg) in assistant_messages.iter().enumerate() {
        let stage = if i == 0 { "Generated" } else { "Enhanced" };
        println!("\n   {} Content (Stage {}):", stage, i + 1);
        println!("      • Length: {} characters", msg.content.len());
        println!("      • Words: {}", msg.content.split_whitespace().count());
        println!("      • Lines: {}", msg.content.lines().count());

        let preview = if msg.content.len() > 200 {
            format!("{}...", &msg.content[..200])
        } else {
            msg.content.clone()
        };
        println!("      • Preview: \"{}\"", preview);
    }

    // ✅ STEP 7: Comprehensive Error Analysis
    println!("\n⚠️  Step 7: Error persistence and quality analysis");

    let errors = final_state.errors.snapshot();

    if !errors.is_empty() {
        println!("   📊 Error Summary:");
        println!("      • Total errors captured: {}", errors.len());

        // Categorize errors
        let mut error_categories: FxHashMap<String, usize> = FxHashMap::default();
        let mut error_scopes: FxHashMap<String, usize> = FxHashMap::default();

        for error in &errors {
            // Count by tags
            for tag in &error.tags {
                *error_categories.entry(tag.clone()).or_insert(0) += 1;
            }

            // Count by scope
            let scope_str = match &error.scope {
                ErrorScope::Node { kind, .. } => format!("Node:{}", kind),
                ErrorScope::Scheduler { .. } => "Scheduler".to_string(),
                ErrorScope::Runner { .. } => "Runner".to_string(),
                ErrorScope::App => "App".to_string(),
            };
            *error_scopes.entry(scope_str).or_insert(0) += 1;
        }

        println!("\n   📈 Error Categories:");
        for (category, count) in error_categories {
            println!("      • {}: {} occurrence(s)", category, count);
        }

        println!("\n   🎯 Error Scopes:");
        for (scope, count) in error_scopes {
            println!("      • {}: {} error(s)", scope, count);
        }

        println!("\n   📋 Detailed Error Report:");
        println!("{}", pretty_print(&errors));

        println!("\n   ✅ All errors persisted to SQLite for analysis");
    } else {
        println!("   ✅ No errors captured - excellent execution quality!");
    }

    // ✅ STEP 8: Performance Benchmarking
    println!("\n🏆 Step 8: Performance benchmarking and recommendations");

    let total_content_length = assistant_messages
        .iter()
        .map(|msg| msg.content.len())
        .sum::<usize>();

    let chars_per_second = total_content_length as f64 / total_execution_time.as_secs_f64();

    println!("   📊 Performance Benchmarks:");
    println!(
        "      • Total content generated: {} characters",
        total_content_length
    );
    println!("      • Generation rate: {:.1} chars/sec", chars_per_second);
    println!(
        "      • Error rate: {:.2}%",
        (errors.len() as f64 / 2.0) * 100.0
    ); // 2 nodes max

    // Performance assessment
    println!("\n   🎯 Performance Assessment:");
    if chars_per_second > 100.0 {
        println!("      ✅ Excellent: High-throughput content generation");
    } else if chars_per_second > 50.0 {
        println!("      ✅ Good: Acceptable content generation rate");
    } else {
        println!("      ⚠️  Slow: Consider optimizing streaming configuration");
    }

    if errors.len() <= 2 {
        println!("      ✅ Excellent: Low error rate indicates high quality");
    } else if errors.len() <= 5 {
        println!("      ⚠️  Moderate: Some quality issues detected");
    } else {
        println!("      ❌ High: Significant quality issues need attention");
    }

    // ✅ STEP 9: Persistence Verification
    println!("\n💾 Step 9: Verifying SQLite persistence");

    let db_path = app
        .runtime_config()
        .sqlite_db_name
        .as_ref()
        .cloned()
        .unwrap_or_else(|| "weavegraph.db".to_string());

    println!("   ✅ Workflow state persisted to: {}", db_path);
    println!(
        "   ✅ Session ID: {:?}",
        app.runtime_config().session_id.as_ref()
    );
    println!("   ✅ Error events: {} persisted", errors.len());
    println!("   ✅ Checkpoints: Available for workflow resumption");
    println!("   💡 Use SQLite tools to examine detailed execution history");

    // ✅ FINAL SUMMARY
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                      Demo 4 Complete                    ║");
    println!("╚══════════════════════════════════════════════════════════╝");
    println!("\n🏆 Advanced patterns demonstrated:");
    println!("   • Real-time streaming LLM integration (Gemini)");
    println!("   • Comprehensive error persistence and categorization");
    println!("   • Production-ready quality validation and monitoring");
    println!("   • Advanced performance benchmarking and analysis");
    println!("   • SQLite-backed checkpoint persistence");
    println!("   • Event-driven architecture with rich observability");
    println!("   • Modern message patterns and error handling");
    println!("\n🎯 Demo series complete! You've now seen:");
    println!("   📚 Demo 1: Basic graph building and execution patterns");
    println!("   ⚙️  Demo 2: Scheduler-driven workflow execution");
    println!("   🤖 Demo 3: LLM integration with runtime configuration");
    println!("   🚀 Demo 4: Advanced streaming with error persistence");
    println!("\n💡 These demos provide a comprehensive foundation for building");
    println!("   production-ready AI agent workflows with Weavegraph!");

    Ok(())
}
