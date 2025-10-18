//! Demo 6: Agent + MCP integration with Graph Runtime
//!
//! This example demonstrates how to:
//! 1. Launch an MCP server as a child process
//! 2. Discover (list) all available MCP tools
//! 3. Attach every tool to an agent builder (rig-core 0.21: single `rmcp_tool` per tool)
//! 4. Stream a multi‑turn response while capturing text + reasoning
//! 5. Gracefully shut down the MCP child process
//!
//! Run with:
//!   cargo run --example demo6_agent_mcp
//!
//! Prereqs:
//!   1. Ollama running locally (`ollama serve`) and model pulled (e.g. `ollama pull gemma3:latest`)
//!   2. Node + npx available to launch the MCP server
//!   3. Network access for any tools that reach out
//!
//! Notes:
//! - We target rig-core 0.21 which only has the singular `.rmcp_tool` API. A future upgrade
//!   (>=0.22) can swap the fold for a single plural call if/when it exists.
//! - This example uses the full Weavegraph graph runtime (Start -> Agent -> End) instead of
//!   directly invoking the node, mirroring other demos for consistency.
//
use async_trait::async_trait;
use chrono::Utc;
use futures_util::StreamExt;
use rmcp::service::ServiceExt;
use rustc_hash::FxHashMap;
use serde_json::json;
use std::process::Stdio;
use tokio::process::Command;

use rig::agent::MultiTurnStreamItem;
use rig::message::{Reasoning, Text};
use rig::prelude::*;
use rig::providers::ollama;
use rig::streaming::{StreamedAssistantContent, StreamingPrompt};

use weavegraph::channels::{pretty_print, ErrorEvent, ErrorScope, LadderError};
use weavegraph::graphs::GraphBuilder;
use weavegraph::message::Message;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::runtimes::{CheckpointerType, EventBusConfig, RuntimeConfig};
use weavegraph::state::{StateSnapshot, VersionedState};
use weavegraph::types::NodeKind;

/// System prompt guiding the agent's behavior.
/// - Keep responses concise and technically accurate
/// - Announce which MCP tools (if any) you are using
/// - Single response (no iterative clarification)
const SYSTEM_PROMPT: &str = r#"You are an expert Rust assistant with access to MCP tools, your main MCP server is designed to search and get documents for rust crates.
When MCP tools relevant to your task are available, announce which of them you're currently using to answer the user prompt.
If no tools are present, proceed gracefully.
Keep answers concise and technically accurate.
You must answer in one shot; there will be no back and forth."#;

/// Node that encapsulates: spawn MCP child process -> list tools -> build agent -> stream output.
struct AgentNode;

#[async_trait]
impl Node for AgentNode {
    async fn run(
        &self,
        snapshot: StateSnapshot,
        ctx: NodeContext,
    ) -> Result<NodePartial, NodeError> {
        ctx.emit(
            "agent",
            "Spawning MCP child process (npx @upstash/context7-mcp)",
        )?;

        // --- Launch MCP child ---------------------------------------------------------------
        // rmcp 0.6: we manually spawn process & bridge stdin/stdout into a transport.
        let mut child = Command::new("npx")
            .arg("-y")
            .arg("@upstash/context7-mcp")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| NodeError::Provider {
                provider: "mcp",
                message: format!("spawn failed: {e}"),
            })?;
        let child_stdin = child.stdin.take().ok_or(NodeError::Provider {
            provider: "mcp",
            message: "failed to take child stdin".into(),
        })?;
        let child_stdout = child.stdout.take().ok_or(NodeError::Provider {
            provider: "mcp",
            message: "failed to take child stdout".into(),
        })?;
        let service =
            ().serve((child_stdout, child_stdin))
                .await
                .map_err(|e| NodeError::Provider {
                    provider: "mcp",
                    message: format!("handshake failed: {e}"),
                })?;
        ctx.emit("agent", "MCP handshake complete")?;

        // --- List all available tools -------------------------------------------------------
        let tool_names = match service.peer().list_tools(Default::default()).await {
            Ok(list) => {
                let names: Vec<String> = list.tools.iter().map(|t| t.name.to_string()).collect();
                if names.is_empty() {
                    ctx.emit("agent", "No MCP tools reported")?;
                } else {
                    ctx.emit("agent", format!("Discovered {} tool(s)", names.len()))?;
                }
                names
            }
            Err(err) => {
                ctx.emit("agent", format!("Failed to list tools: {err}"))?;
                Vec::new()
            }
        };

        // Fetch full tool objects again (separate call for full struct usage).
        let tools = service
            .list_tools(Default::default())
            .await
            .map_err(|_| NodeError::Provider {
                provider: "mcp",
                message: "could not list context7 tools".into(),
            })?
            .tools;

        // --- Build agent with each tool (fold pattern for rig-core 0.21) --------------------
        let peer = service.peer().clone();
        let ollama_client = ollama::Client::new();
        let builder = tools.into_iter().fold(
            ollama_client
                .agent("qwen3:4b-instruct-2507-q4_K_M") // Model name can be swapped (e.g. gemma3)
                .preamble(SYSTEM_PROMPT)
                .temperature(0.2),
            |b, tool| b.rmcp_tool(tool, peer.clone()),
        );
        let agent = builder.build();

        // --- Prepare prompt (first user message) --------------------------------------------
        let prompt = snapshot
            .messages
            .first()
            .map(|m| m.content.as_str())
            .unwrap();

        ctx.emit("agent", "Streaming multi-turn response")?;
        let stream_start = std::time::Instant::now();

        // multi_turn(2): instructs streaming to allow limited internal turns (if supported).
        let mut stream = agent.stream_prompt(prompt).multi_turn(2).await;

        // Accumulators
        let mut errors = Vec::new();
        let mut accumulated = String::new();
        let mut reasoning_total = String::new();
        let mut chunk_index = 0usize;

        // --- Streaming loop ----------------------------------------------------------------
        while let Some(item) = stream.next().await {
            match item {
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Text(Text {
                    text,
                }))) => {
                    chunk_index += 1;
                    accumulated.push_str(&text);
                    if chunk_index % 5 == 1 {
                        ctx.emit(
                            "agent",
                            format!(
                                "Received text chunk {} ({} chars total)",
                                chunk_index,
                                accumulated.len()
                            ),
                        )?;
                    }
                }
                Ok(MultiTurnStreamItem::StreamItem(StreamedAssistantContent::Reasoning(
                    Reasoning { reasoning, .. },
                ))) => {
                    let rtxt = reasoning.join("\n");
                    ctx.emit("agent", "Received reasoning segment")?;
                    reasoning_total.push_str(&rtxt);
                }
                Ok(MultiTurnStreamItem::FinalResponse(resp)) => {
                    accumulated.push_str(resp.response());
                    ctx.emit(
                        "agent",
                        format!("Final response received ({} chars)", resp.response().len()),
                    )?;
                }
                Ok(_) => { /* ignore other streamed item variants */ }
                Err(err) => {
                    errors.push(ErrorEvent {
                        when: Utc::now(),
                        scope: ErrorScope::Node {
                            kind: "Ollama streaming error".into(),
                            step: ctx.step,
                        },
                        error: LadderError {
                            message: err.to_string(),
                            cause: None,
                            details: json!("stream_failure"),
                        },
                        tags: vec!["ollama".into(), "stream_error".into()],
                        context: json!({"severity": "error"}),
                    });
                    break;
                }
            }
        }

        ctx.emit(
            "agent",
            format!(
                "Streaming complete in {:.2}s ({} text chars, {} reasoning chars)",
                stream_start.elapsed().as_secs_f64(),
                accumulated.len(),
                reasoning_total.len()
            ),
        )?;

        // --- Graceful shutdown --------------------------------------------------------------
        let _ = service.cancel().await; // protocol-level cancel

        if let Some(pid) = child.id() {
            ctx.emit(
                "agent",
                format!("Initiating MCP child shutdown (pid={pid})"),
            )?;
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                ctx.emit("agent", format!("MCP child already exited: {status}"))?;
            }
            Ok(None) => {
                use tokio::time::{sleep, Duration};
                sleep(Duration::from_millis(200)).await;
                match child.try_wait() {
                    Ok(Some(status)) => {
                        ctx.emit("agent", format!("MCP child exited after grace: {status}"))?;
                    }
                    Ok(None) => {
                        ctx.emit("agent", "Force killing MCP child process")?;
                        if let Err(err) = child.kill().await {
                            ctx.emit("agent", format!("Failed to kill MCP process: {err}"))?;
                        } else {
                            let _ = child.wait().await;
                        }
                    }
                    Err(err) => {
                        errors.push(ErrorEvent {
                            when: Utc::now(),
                            scope: ErrorScope::Node {
                                kind: "MCP shutdown".into(),
                                step: ctx.step,
                            },
                            error: LadderError {
                                message: "Error re-checking MCP process status".into(),
                                cause: None,
                                details: json!(err.to_string()),
                            },
                            tags: vec!["mcp".into(), "shutdown".into()],
                            context: json!({"severity": "warning"}),
                        });
                    }
                }
            }
            Err(err) => {
                errors.push(ErrorEvent {
                    when: Utc::now(),
                    scope: ErrorScope::Node {
                        kind: "MCP shutdown".into(),
                        step: ctx.step,
                    },
                    error: LadderError {
                        message: "Error querying MCP child process".into(),
                        cause: None,
                        details: json!(err.to_string()),
                    },
                    tags: vec!["mcp".into(), "shutdown".into()],
                    context: json!({"severity": "warning"}),
                });
            }
        }

        // --- Build NodePartial --------------------------------------------------------------
        let mut extra = FxHashMap::default();
        extra.insert("mcp_tool_names".into(), json!(tool_names));
        extra.insert("reasoning_length".into(), json!(reasoning_total.len()));
        extra.insert(
            "total tokens returned from agent".into(),
            json!(chunk_index),
        );

        let assistant_msg = if accumulated.is_empty() {
            Message::assistant("No response produced.")
        } else {
            Message::assistant(&accumulated)
        };

        Ok(NodePartial::new()
            .with_messages(vec![assistant_msg])
            .with_extra(extra)
            .with_errors(errors))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Demo 6: Agent + MCP Tools ===");

    // Initial user query (single-pass request)
    let initial_state = VersionedState::new_with_user_message(
        "Fetch detailed docs for these two Rust crates: axum and actix, then compare their features and return a list of the 5 major differences between them. Do all of this in one go.",
    );

    // Runtime config: named session + SQLite checkpointer
    let runtime_config = RuntimeConfig {
        session_id: Some("mcp_demo".to_string()),
        checkpointer: Some(CheckpointerType::SQLite),
        sqlite_db_name: Some("weavegraph_demo6.db".to_string()),
        event_bus: EventBusConfig::with_stdout_only(),
    };

    // Graph: Start -> (Mcp Agent) -> End
    let app = GraphBuilder::new()
        .add_node(NodeKind::Custom("Mcp Agent".into()), AgentNode)
        .add_edge(NodeKind::Start, NodeKind::Custom("Mcp Agent".into()))
        .add_edge(NodeKind::Custom("Mcp Agent".into()), NodeKind::End)
        .with_runtime_config(runtime_config)
        .compile()
        .map_err(|e| miette::miette!("graph build failed: {e}"))?;

    let final_state = app
        .invoke(initial_state)
        .await
        .map_err(|e| miette::miette!("LLM workflow execution failed: {e}"))?;

    let snapshot = final_state.snapshot();

    // Assistant messages
    for (i, m) in snapshot.messages.iter().enumerate() {
        if m.has_role(Message::ASSISTANT) {
            println!("\n[Assistant Message {}]\n{}\n", i + 1, m.content);
        }
    }

    // Extra metadata
    if !snapshot.extra.is_empty() {
        println!("--- Extra Metadata ---");
        for (k, v) in &snapshot.extra {
            println!("{k}: {v}");
        }
    }

    // Collected errors / warnings
    let errors = snapshot.errors;
    if !errors.is_empty() {
        println!("\n⚠️  Errors encountered during generation:");
        println!("{}", pretty_print(&errors));
    } else {
        println!("\n🎯 Content generation completed successfully!");
    }

    println!("\n=== Demo Complete ===");
    Ok(())
}
