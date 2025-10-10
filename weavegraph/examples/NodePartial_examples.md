# NodePartial Construction Examples

This document demonstrates the streamlined `NodePartial` API in action, showing how the "one clear way to do each task" philosophy creates readable, maintainable code.

## Key Patterns Demonstrated

- **Fatal vs Recoverable Errors**: Use `Err(NodeError)` for fatal errors that stop workflow execution, `NodePartial.errors` for recoverable ones that get logged but allow processing to continue
- **Fluent API Construction**: Use the fluent pattern `NodePartial::new().with_messages().with_extra().with_errors()` to build responses in a readable, chainable way
- **Conditional Returns**: Different scenarios can return different combinations of data without API bloat
- **Error Accumulation**: Collect multiple errors during processing and return them all in a single response
- **Rich Context**: Use extra data to provide detailed metadata about the processing results

## 1. Basic Processing Errors

This example shows the fundamental distinction between fatal and recoverable errors - a key design principle that prevents workflow chaos.

```rust
use weavegraph::node::{Node, NodeContext, NodePartial, NodeError};
use weavegraph::channels::errors::ErrorEvent;
use weavegraph::state::StateSnapshot;
use async_trait::async_trait;

struct ValidationNode;

#[async_trait]
impl Node for ValidationNode {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        // Fatal error - stops workflow execution immediately
        // Use this for unrecoverable conditions
        if snapshot.messages.is_empty() {
            return Err(NodeError::ValidationFailed("No messages to process".to_string()));
        }

        // Recoverable error - logged but workflow continues
        // Use this for issues that can be worked around
        if snapshot.extra.get("user_id").is_none() {
            let error = ErrorEvent {
                scope: "validation".to_string(),
                message: "Missing user_id, using default".to_string(),
                ..Default::default()
            };
            // Notice: ONE clear way to return just errors
            return Ok(NodePartial::new().with_errors(vec![error]));
        }

        Ok(NodePartial::default())
    }
}
```

**Key insight**: The API makes the distinction between fatal and recoverable errors explicit in the return type.


## 2. Multi-part Processing with Errors and Data

This demonstrates how to handle batch processing where some items succeed and others fail - a common real-world scenario.

```rust
use weavegraph::utils::collections::new_extra_map;
use serde_json::json;

struct DataProcessorNode;

#[async_trait]
impl Node for DataProcessorNode {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        ctx.emit("processing", "Starting data processing")?;

        let mut messages = Vec::new();
        let mut errors = Vec::new();
        let mut extra = new_extra_map();

        // Process multiple data items - some may fail
        for (index, message) in snapshot.messages.iter().enumerate() {
            match self.process_item(message) {
                Ok(result) => {
                    messages.push(Message::assistant(&format!("Processed: {}", result)));
                    extra.insert(format!("result_{}", index), json!(result));
                }
                Err(e) => {
                    // Accumulate errors but keep processing
                    errors.push(ErrorEvent {
                        scope: "processing".to_string(),
                        message: format!("Failed to process item {}: {}", index, e),
                        ..Default::default()
                    });
                }
            }
        }

        // The streamlined way to combine multiple aspects
        Ok(NodePartial::new()
            .with_messages(messages)
            .with_extra(extra)
            .with_errors(errors))
    }
}

impl DataProcessorNode {
    fn process_item(&self, message: &Message) -> Result<String, &'static str> {
        if message.content.is_empty() {
            Err("Empty content")
        } else {
            Ok(format!("Processed: {}", message.content))
        }
    }
}
```

**Key insight**: The fluent API pattern with `NodePartial::new().with_*()` makes complex combinations readable and explicit. No magic, no hidden complexity.


## 3. Complete Multi-faceted Response

This shows a comprehensive node that returns messages, rich metadata, and conditional warnings - demonstrating the full power of the streamlined API.

```rust
struct ComprehensiveAnalyzerNode {
    analysis_type: String,
}

#[async_trait]
impl Node for ComprehensiveAnalyzerNode {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        ctx.emit("analysis", format!("Starting {} analysis", self.analysis_type))?;

        // Analyze the conversation
        let analysis_result = self.analyze_conversation(&snapshot.messages);

        // Create response messages
        let messages = vec![
            Message::assistant(&format!("Analysis type: {}", self.analysis_type)),
            Message::assistant(&format!("Summary: {}", analysis_result.summary)),
            Message::system(&format!("Confidence: {:.2}", analysis_result.confidence)),
        ];

        // Create rich metadata
        let mut extra = new_extra_map();
        extra.insert("analysis_type".to_string(), json!(self.analysis_type));
        extra.insert("confidence_score".to_string(), json!(analysis_result.confidence));
        extra.insert("word_count".to_string(), json!(analysis_result.word_count));
        extra.insert("sentiment".to_string(), json!(analysis_result.sentiment));
        extra.insert("timestamp".to_string(), json!(chrono::Utc::now().to_rfc3339()));

        // Create conditional warnings based on analysis quality
        let mut errors = Vec::new();
        if analysis_result.confidence < 0.5 {
            errors.push(ErrorEvent {
                scope: "analysis".to_string(),
                message: "Low confidence analysis - results may be unreliable".to_string(),
                ..Default::default()
            });
        }

        if analysis_result.word_count < 10 {
            errors.push(ErrorEvent {
                scope: "data_quality".to_string(),
                message: "Insufficient text for reliable analysis".to_string(),
                ..Default::default()
            });
        }

        // Combine everything - notice how readable this is
        ctx.emit("completed", "Analysis completed successfully")?;
        
        if !errors.is_empty() {
            Ok(NodePartial::new()
                .with_messages(messages)
                .with_extra(extra)
                .with_errors(errors))
        } else {
            Ok(NodePartial::new()
                .with_messages(messages)
                .with_extra(extra))
        }
    }
}

struct AnalysisResult {
    summary: String,
    confidence: f64,
    word_count: usize,
    sentiment: String,
}

impl ComprehensiveAnalyzerNode {
    fn analyze_conversation(&self, messages: &[Message]) -> AnalysisResult {
        let total_words: usize = messages.iter()
            .map(|m| m.content.split_whitespace().count())
            .sum();

        AnalysisResult {
            summary: "Conversation analyzed".to_string(),
            confidence: if total_words > 50 { 0.85 } else { 0.45 },
            word_count: total_words,
            sentiment: "neutral".to_string(),
        }
    }
}
```

**Key insight**: Even complex responses remain readable because there's only one way to build each aspect. No cognitive load about which builder method to use.


## 4. Conditional Processing with Different Return Types

This example shows how different execution paths can return completely different types of data without any API complexity.

```rust
struct ConditionalProcessorNode {
    mode: ProcessingMode,
}

enum ProcessingMode {
    Fast,
    Thorough,
    ErrorTesting,
}

#[async_trait]
impl Node for ConditionalProcessorNode {
    async fn run(&self, snapshot: StateSnapshot, ctx: NodeContext) -> Result<NodePartial, NodeError> {
        match self.mode {
            ProcessingMode::Fast => {
                // Minimal response - just messages, no extras
                // Notice: ONE clear constructor for this case
                Ok(NodePartial::new().with_messages(vec![
                    Message::assistant("Fast processing complete")
                ]))
            }

            ProcessingMode::Thorough => {
                // Rich response with messages and detailed metadata
                let mut extra = new_extra_map();
                extra.insert("processing_time_ms".to_string(), json!(150));
                extra.insert("items_processed".to_string(), json!(snapshot.messages.len()));
                extra.insert("mode".to_string(), json!("thorough"));

                Ok(NodePartial::new()
                    .with_messages(vec![
                        Message::assistant("Thorough processing complete"),
                        Message::system("All validation checks passed"),
                        )
                    .with_extra(extra)
                )
            }

            ProcessingMode::ErrorTesting => {
                // Demonstrate error handling with partial success
                let errors = vec![
                    ErrorEvent {
                        scope: "test".to_string(),
                        message: "Simulated processing warning".to_string(),
                        ..Default::default()
                    },
                    ErrorEvent {
                        scope: "test".to_string(),
                        message: "Another test warning".to_string(),
                        ..Default::default()
                    },
                ];

                // Start with errors, then add messages
                Ok(NodePartial::new()
                    .with_errors(errors)
                    .with_messages(vec![
                        Message::assistant("Error testing mode - warnings generated")
                ]))
            }
        }
    }
}
```

**Key insight**: Different scenarios require different data - the streamlined API handles this naturally without forcing you to choose between multiple builder patterns.

---

## Why This Approach Works

The examples above demonstrate that **removing complexity from the API** doesn't limit functionality - it clarifies it. Each example shows:

1. **One obvious way** to create each type of response using the fluent API
2. **Readable chaining** with `NodePartial::new().with_*()` for combining aspects
3. **Clear intent** at every step with explicit method calls
4. **No choice paralysis** - one consistent pattern for all scenarios
