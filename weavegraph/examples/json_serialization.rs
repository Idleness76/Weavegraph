//! JSON Serialization Demo
//!
//! Demonstrates the JSON serialization capabilities of Weavegraph events:
//! - `to_json_value()` - Structured JSON with normalized schema
//! - `to_json_string()` - Compact JSON string
//! - `to_json_pretty()` - Pretty-printed JSON for debugging
//! - `JsonLinesSink` - File and stdout logging
//!
//! ## Run it
//! ```bash
//! cargo run --example json_serialization
//! ```

use rustc_hash::FxHashMap;
use serde_json::Value;
use std::fs;
use tempfile::NamedTempFile;
use weavegraph::event_bus::{Event, EventBus, JsonLinesSink, LLMStreamingEvent};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== JSON Serialization Demo ===\n");

    // Create sample events
    let node_event = Event::node_message_with_meta("router", 5, "routing", "Processing request");
    let diagnostic_event = Event::diagnostic("system", "Service ready");

    let mut llm_metadata = FxHashMap::default();
    llm_metadata.insert("model".to_string(), Value::String("gpt-4".to_string()));
    llm_metadata.insert("temperature".to_string(), Value::from(0.7));

    let llm_event = Event::LLM(LLMStreamingEvent::chunk_event(
        Some("session-123".to_string()),
        Some("node-abc".to_string()),
        Some("stream-xyz".to_string()),
        "Hello, world!",
        llm_metadata,
    ));

    // Demonstrate to_json_value()
    println!("1. to_json_value() - Structured JSON Value");
    println!("-------------------------------------------");
    let node_json = node_event.to_json_value();
    println!("Node Event:");
    println!("  type: {}", node_json["type"]);
    println!("  scope: {}", node_json["scope"]);
    println!("  message: {}", node_json["message"]);
    println!("  metadata.node_id: {}", node_json["metadata"]["node_id"]);
    println!("  metadata.step: {}", node_json["metadata"]["step"]);
    println!();

    let llm_json = llm_event.to_json_value();
    println!("LLM Event:");
    println!("  type: {}", llm_json["type"]);
    println!(
        "  metadata.session_id: {}",
        llm_json["metadata"]["session_id"]
    );
    println!(
        "  metadata.stream_id: {}",
        llm_json["metadata"]["stream_id"]
    );
    println!("  metadata.is_final: {}", llm_json["metadata"]["is_final"]);
    println!("  metadata.model: {}", llm_json["metadata"]["model"]);
    println!();

    // Demonstrate to_json_string()
    println!("2. to_json_string() - Compact JSON");
    println!("-----------------------------------");
    let compact = diagnostic_event.to_json_string()?;
    println!("Diagnostic Event (compact):");
    println!("{}", compact);
    println!();

    // Demonstrate to_json_pretty()
    println!("3. to_json_pretty() - Pretty-printed JSON");
    println!("------------------------------------------");
    let pretty = node_event.to_json_pretty()?;
    println!("Node Event (pretty):");
    println!("{}", pretty);
    println!();

    // Demonstrate JsonLinesSink to file
    println!("4. JsonLinesSink - File Output");
    println!("-------------------------------");
    let temp_file = NamedTempFile::new()?;
    let temp_path = temp_file.path().to_path_buf();
    println!("Writing events to: {:?}", temp_path);

    {
        let sink = JsonLinesSink::to_file(&temp_path)?;
        let bus = EventBus::with_sinks(vec![Box::new(sink)]);
        bus.listen_for_events();
        let emitter = bus.get_emitter();

        // Emit some events
        emitter.emit(node_event.clone())?;
        emitter.emit(diagnostic_event.clone())?;
        emitter.emit(llm_event.clone())?;

        // Give listener time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    // Read back and display
    let contents = fs::read_to_string(&temp_path)?;
    println!("\nFile contents (JSON Lines format):");
    for (i, line) in contents.lines().enumerate() {
        println!("Line {}: {}", i + 1, line);
    }
    println!();

    // Demonstrate JsonLinesSink to stdout
    println!("5. JsonLinesSink - Stdout Output");
    println!("---------------------------------");
    println!("Streaming 3 events to stdout:\n");

    {
        let sink = JsonLinesSink::to_stdout();
        let bus = EventBus::with_sinks(vec![Box::new(sink)]);
        bus.listen_for_events();
        let emitter = bus.get_emitter();

        emitter.emit(Event::diagnostic("demo", "Starting..."))?;
        emitter.emit(Event::node_message("processing", "Working on task"))?;
        emitter.emit(Event::diagnostic("demo", "Complete!"))?;

        // Give listener time to process
        tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
    }

    println!("\n\n6. Comparison: Default Serialization vs JSON Methods");
    println!("----------------------------------------------------");

    // Show the difference
    let default_json = serde_json::to_string_pretty(&node_event)?;
    let normalized_json = node_event.to_json_pretty()?;

    println!("Default Event serialization (raw structure):");
    println!("{}", default_json);
    println!();

    println!("Normalized JSON schema (to_json_pretty):");
    println!("{}", normalized_json);
    println!();

    println!("Notice how to_json_value() provides:");
    println!("  ✓ Consistent 'type' field across all variants");
    println!("  ✓ Normalized 'metadata' object");
    println!("  ✓ Timestamp in ISO 8601 format");
    println!("  ✓ Simplified scope/message extraction");
    println!();

    println!("=== Demo Complete ===");
    Ok(())
}
