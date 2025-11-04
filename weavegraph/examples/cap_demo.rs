//! Simple Ollama Streaming Demo
//!
//! This demo shows:
//! 1. Basic content generation with user input
//! 2. Ollama streaming integration for content refinement
//! 3. Simple graph execution with conditional loops
//!
//! Run with: `cargo run --example cap_demo`
//! Prereqs:
//!   1. `docker run -d --name ollama -p 11434:11434 ollama/ollama` (or `ollama serve` locally)
//!   2. `ollama pull gemma3:latest`
//!   3. Ensure the Ollama service is reachable at `http://localhost:11434`

use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use miette::Result;
use rig::agent::MultiTurnStreamItem;
use rig::message::Text;
use rig::prelude::*;
use rig::{
    providers::ollama,
    streaming::{StreamedAssistantContent, StreamingPrompt},
};
use rustc_hash::FxHashMap;
use serde_json::json;
use std::sync::Arc;
use tracing::{info, instrument};
use tracing_error::ErrorLayer;
use tracing_subscriber::{EnvFilter, Layer, layer::SubscriberExt, util::SubscriberInitExt};

use weavegraph::channels::Channel;
use weavegraph::channels::errors::{ErrorEvent, ErrorScope, LadderError, pretty_print};
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

#[derive(Clone)]
struct InputBootstrapperNode;

#[async_trait]
impl Node for InputBootstrapperNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("bootstrap_start", "Creating initial content")?;

        let user_input = snapshot
            .messages
            .iter()
            .find(|msg| msg.has_role(Message::USER))
            .map(|msg| msg.content.as_str())
            .unwrap_or("Tell me about Weavegraph capabilities");

        ctx.emit(
            "bootstrap_user_input",
            format!("User input: {}", user_input),
        )?;

        let initial_content = format!(
            "Here's a brief overview based on your request:
            {}\n\nWeavegraph is a Rust-based graph execution framework that provides:
            \n• Flexible node orchestration
            \n• Built-in state management
            \n\nThis content can be refined further.",
            user_input
        );

        ctx.emit("bootstrap_content", "Generated initial content")?;

        let mut extra = FxHashMap::default();
        extra.insert("needs_more_refinement".into(), json!(true));

        Ok(NodePartial::new()
            .with_messages(vec![Message::assistant(&initial_content)])
            .with_extra(extra))
    }
}

#[derive(Clone)]
struct OllamaIterativeRefinerNode {
    max_iterations: usize,
}

#[async_trait]
impl Node for OllamaIterativeRefinerNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("refine_start", "Evaluating latest draft")?;

        let latest = snapshot
            .messages
            .iter()
            .rev()
            .find(|m| m.has_role(Message::ASSISTANT))
            .map(|m| m.content.clone())
            .unwrap_or_else(|| "No previous draft; using bootstrap.".into());

        let current_iter = snapshot
            .extra
            .get("iteration_count")
            .and_then(|value| value.as_u64())
            .map(|value| value as usize)
            .unwrap_or(0);

        if current_iter >= self.max_iterations {
            ctx.emit(
                "refine_skip",
                format!(
                    "Iteration limit reached ({}); passing draft through",
                    self.max_iterations
                ),
            )?;

            let mut extra = FxHashMap::default();
            extra.insert("iteration_count".into(), json!(current_iter));
            extra.insert("refiner_last_action".into(), json!("iteration_cap_reached"));
            extra.insert("needs_more_refinement".into(), json!(false));

            return Ok(NodePartial {
                messages: Some(vec![Message::assistant(&latest)]),
                extra: Some(extra),
                errors: None,
                frontier: None,
            });
        }

        let mut extra = FxHashMap::default();
        let mut errors = Vec::new();

        let refined = {
            ctx.emit(
                "refine_mode",
                format!(
                    "Iteration {}/{} using local Ollama streaming",
                    current_iter + 1,
                    self.max_iterations
                ),
            )?;

            let agent = ollama::Client::new()
                .agent("gemma3:latest")
                .preamble(
                    "You are a technical content specialist. When improving content, make meaningful enhancements: add specific details, improve structure, provide concrete examples, and enhance readability. Avoid generic improvements.",
                )
                .temperature(0.7)
                .build();

            let refinement_goals = match current_iter {
                0 => "expand on the key benefits and add concrete examples",
                1 => "make it more engaging with better structure and flow",
                _ => "polish the language and ensure clarity",
            };

            let prompt = format!(
                "This is iteration {} of content refinement. Your goal: {}.\n\nCurrent content:\n{}\n\nProvide an improved version that is more detailed and engaging:",
                current_iter + 1,
                refinement_goals,
                latest
            );

            let mut stream = agent.stream_prompt(prompt).await;
            let mut combined = String::new();
            let mut chunk_index = 0usize;
            let mut char_total = 0usize;
            let mut stream_failed = false;
            let mut final_response: Option<String> = None;

            while let Some(item) = stream.next().await {
                match item {
                    Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                        text,
                    }))) => {
                        chunk_index += 1;
                        char_total += text.len();
                        if chunk_index == 1 || chunk_index % 10 == 0 {
                            ctx.emit(
                                "refine_progress",
                                format!(
                                    "Streaming totals: {chunk_index} chunk(s) / {char_total} character(s)"
                                ),
                            )?;
                        }
                        if chunk_index == 1 {
                            extra.insert(
                                "refiner_first_chunk_preview".into(),
                                json!(text.chars().take(120).collect::<String>()),
                            );
                        }
                        combined.push_str(&text);
                    }
                    Ok(MultiTurnStreamItem::FinalResponse(response)) => {
                        final_response = Some(response.response().to_string());
                        break;
                    }
                    Err(err) => {
                        stream_failed = true;
                        ctx.emit("refine_error", format!("Ollama streaming error: {err}"))?;
                        errors.push(ErrorEvent {
                            when: Utc::now(),
                            scope: ErrorScope::Node {
                                kind: "OllamaIterativeRefiner".into(),
                                step: ctx.step,
                            },
                            error: LadderError {
                                message: format!("Ollama streaming error: {err}"),
                                cause: None,
                                details: json!({
                                    "iteration": current_iter + 1,
                                    "mode": "stream_prompt"
                                }),
                            },
                            tags: vec!["ollama".into(), "streaming_error".into()],
                            context: json!({"severity": "error"}),
                        });
                        break;
                    }
                    _ => {}
                }
            }

            extra.insert("refiner_model".into(), json!("ollama:gemma3:latest"));
            extra.insert("refiner_chunk_count".into(), json!(chunk_index));
            extra.insert("refiner_char_total".into(), json!(char_total));

            // Use the final_response if available, otherwise use the streamed content
            if let Some(response_text) = final_response {
                if !response_text.trim().is_empty() {
                    combined = response_text;
                }
            }

            ctx.emit(
                "refine_totals",
                format!(
                    "Iteration {} totals: {} chunk(s) / {} character(s)",
                    current_iter + 1,
                    chunk_index,
                    char_total
                ),
            )?;

            if stream_failed || combined.trim().is_empty() {
                ctx.emit(
                    "refine_fallback",
                    "Streaming unavailable; generating deterministic summary",
                )?;
                ctx.emit(
                    "refine_warning",
                    "Ollama returned no content; switching to fallback",
                )?;
                errors.push(ErrorEvent {
                    when: Utc::now(),
                    scope: ErrorScope::Node {
                        kind: "OllamaIterativeRefiner".into(),
                        step: ctx.step,
                    },
                    error: LadderError {
                        message: "Ollama returned empty response".into(),
                        cause: None,
                        details: json!({ "iteration": current_iter + 1 }),
                    },
                    tags: vec!["ollama".into(), "empty_response".into()],
                    context: json!({"severity": "warning"}),
                });

                "Weavegraph is a powerful Rust-based graph execution framework that provides flexible node orchestration, built-in state management, and seamless integration with external services like Ollama for AI-powered content processing.".to_string()
            } else {
                combined.trim().to_string()
            }
        };

        let completed_iterations = current_iter.saturating_add(1);
        let needs_more = completed_iterations < self.max_iterations;

        extra.insert("iteration_count".into(), json!(completed_iterations));
        extra.insert(
            "refiner_last_action".into(),
            json!(if needs_more {
                "content_updated"
            } else {
                "complete"
            }),
        );
        extra.insert("needs_more_refinement".into(), json!(needs_more));

        Ok(NodePartial {
            messages: Some(vec![Message::assistant(&refined)]),
            extra: Some(extra),
            errors: if errors.is_empty() {
                None
            } else {
                Some(errors)
            },
            frontier: None,
        })
    }
}

#[derive(Clone)]
#[allow(dead_code)]
struct SummaryPublisherNode;

#[async_trait]
impl Node for SummaryPublisherNode {
    #[instrument(skip(self, snapshot, ctx), fields(step = ctx.step))]
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit("publish_start", "Finalizing content")?;

        let final_content = snapshot
            .messages
            .iter()
            .rev()
            .find(|m| m.has_role(Message::ASSISTANT))
            .map(|msg| msg.content.clone())
            .unwrap_or_else(|| "No content available.".into());

        ctx.emit("publish_complete", "Content finalized")?;

        Ok(NodePartial::new().with_messages(vec![Message::assistant(&final_content)]))
    }
}

fn init_tracing() {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_filter(
                    EnvFilter::from_default_env()
                        .add_directive("weavegraph=info".parse().unwrap())
                        .add_directive("cap_demo=info".parse().unwrap()),
                ),
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

    info!("\n=== Capstone Demo ===\n");

    let initial_state = VersionedState::builder()
        .with_user_message("The team needs a short update on Weavegraph's capabilities.")
        .build();

    let refinement_predicate: EdgePredicate = Arc::new(|snapshot: StateSnapshot| {
        let needs_refinement = snapshot
            .extra
            .get("needs_more_refinement")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        if needs_refinement {
            vec!["refiner".to_string()]
        } else {
            vec!["End".to_string()]
        }
    });

    info!("🔗 Building Ollama streaming workflow with iterative refinement");

    let app = GraphBuilder::new()
        .add_node(
            NodeKind::Custom("bootstrapper".into()),
            InputBootstrapperNode,
        )
        .add_node(
            NodeKind::Custom("refiner".into()),
            OllamaIterativeRefinerNode { max_iterations: 2 },
        )
        // End node is virtual; no concrete registration needed
        .add_edge(NodeKind::Start, NodeKind::Custom("bootstrapper".into()))
        .add_edge(
            NodeKind::Custom("bootstrapper".into()),
            NodeKind::Custom("refiner".into()),
        )
        .add_conditional_edge(
            NodeKind::Custom("refiner".into()),
            Arc::clone(&refinement_predicate),
        )
        .compile()?;

    info!("   ✓ Ollama streaming workflow compiled");
    info!("   ✓ Pipeline: Bootstrapper → Refiner (iterative) → End");
    info!("   ✓ Conditional looping enabled");
    info!("   ✓ Real-time Ollama streaming configured\n");

    let final_state = app.invoke(initial_state).await?;
    let snapshot = final_state.snapshot();

    // Display only the final refined content from Ollama
    if let Some(latest) = snapshot
        .messages
        .iter()
        .rev()
        .find(|m| m.has_role(Message::ASSISTANT))
    {
        info!("┌────────────────────────────────────────────────────────────────────────────────┐");
        info!("│                           � FINAL WEAVEGRAPH OVERVIEW                        │");
        info!("└────────────────────────────────────────────────────────────────────────────────┘");
        info!("");

        // Clean up the content by removing any iteration mentions or meta-commentary
        let clean_content = latest
            .content
            .lines()
            .filter(|line| {
                !line.contains("iteration")
                    && !line.contains("incorporating your feedback")
                    && !line.contains("To help me refine")
                    && !line.contains("could you tell me")
                    && !line.contains("Notes on Changes")
            })
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string();

        // Find the main content (skip any meta-commentary at the start)
        let main_content = if let Some(start) = clean_content.find("**Weavegraph:") {
            &clean_content[start..]
        } else if let Some(start) = clean_content.find("Weavegraph is") {
            &clean_content[start..]
        } else {
            &clean_content
        };

        // Print the cleaned content
        for line in main_content.lines() {
            if line.trim().starts_with("---") {
                info!("   {}", "═".repeat(76));
            } else if line.trim().starts_with("**") && line.trim().ends_with("**") {
                info!("   📋 {}", line.trim_matches('*').trim());
            } else if line.trim().starts_with("* **") {
                info!("   • {}", line.trim_start_matches("* "));
            } else if line.trim().starts_with("*") && !line.trim().starts_with("**") {
                info!("     ○ {}", line.trim_start_matches("* "));
            } else if !line.trim().is_empty() {
                info!("   {}", line);
            } else {
                info!("");
            }
        }

        info!("");
        info!("┌────────────────────────────────────────────────────────────────────────────────┐");

        // Show summary stats
        let iterations = snapshot
            .extra
            .get("iteration_count")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let char_count = snapshot
            .extra
            .get("refiner_char_total")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let model = snapshot
            .extra
            .get("refiner_model")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        info!(
            "│ ✨ Generated via {} refinement iterations using {}     │",
            iterations, model
        );
        info!(
            "│ 📊 Final content: {} characters                                              │",
            char_count
        );
        info!("└────────────────────────────────────────────────────────────────────────────────┘");
    } else {
        info!("❌ No final content generated");
    }

    let errors = final_state.errors.snapshot();
    if !errors.is_empty() {
        info!("\n⚠️  Errors encountered during generation:");
        info!("{}", pretty_print(&errors));
    } else {
        info!("\n🎯 Content generation completed successfully!");
    }

    Ok(())
}
