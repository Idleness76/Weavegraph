# wg-bastion Architecture

**Version:** 1.0  
**Last Updated:** January 2026  
**Status:** Living Document (Sprint 1)

This document describes the technical architecture of wg-bastion, including module design, integration patterns, and implementation guidelines.

---

## Table of Contents

1. [Design Principles](#design-principles)
2. [System Architecture](#system-architecture)
3. [Module Deep Dive](#module-deep-dive)
4. [Integration with weavegraph](#integration-with-weavegraph)
5. [Data Flow](#data-flow)
6. [Configuration Management](#configuration-management)
7. [Extension Points](#extension-points)
8. [Performance Considerations](#performance-considerations)

---

## Design Principles

### 1. Defense in Depth

Multiple layered controls ensure no single failure compromises security:

```
Input Layer ─► Prompt Layer ─► Execution Layer ─► Output Layer
     │              │               │                  │
     └──────────────┴───────────────┴──────────────────┴─► Telemetry
```

### 2. Zero-Trust Architecture

- Validate all inputs (user prompts, RAG documents, tool responses)
- Verify all outputs (LLM responses, tool results)
- Audit all actions (tool calls, agent decisions)
- Trust nothing by default

### 3. Graceful Degradation

`FailMode` configuration allows flexible responses:

- **Closed**: Block threats (production default)
- **Open**: Log threats but allow (testing/staging)
- **LogOnly**: Audit mode (monitoring only)

### 4. Minimal Latency

- Heuristic-first detection (<10ms)
- Optional ML classifiers (configurable)
- Parallel stage execution where possible
- Streaming support for large payloads

### 5. Composability

- Pipeline stages are independent and reusable
- Modules can be used standalone or combined
- Custom stages can be added via trait implementation

---

## System Architecture

### High-Level Components

```text
┌─────────────────────────────────────────────────────────────┐
│ Application Layer (weavegraph App)                          │
│  ├─► GraphBuilder::with_security_policy(policy)            │
│  ├─► App::invoke() / invoke_streaming()                    │
│  └─► EventBus integration for telemetry                    │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ wg-bastion Security Layer                                   │
│                                                             │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────┐     │
│  │ Config       │  │ Pipeline     │  │ Telemetry    │     │
│  │ Management   │  │ Orchestration│  │ & Audit      │     │
│  └──────┬───────┘  └──────┬───────┘  └──────┬───────┘     │
│         │                 │                  │              │
│  ┌──────┴─────────────────┴──────────────────┴───────┐     │
│  │ Security Modules (input/output/tools/rag/etc.)    │     │
│  └────────────────────────────────────────────────────┘     │
└─────────────────────────────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────┐
│ External Services (Optional)                                │
│  ├─► LLM Providers (OpenAI, etc.)                          │
│  ├─► Presidio API (PII detection)                          │
│  ├─► ONNX Runtime (ML classifiers)                         │
│  ├─► Redis (distributed rate limiting)                     │
│  └─► OTLP Collector (observability)                        │
└─────────────────────────────────────────────────────────────┘
```

---

## Module Deep Dive

### Core Modules (Foundation)

#### `config` – Policy Configuration

**Purpose**: Load and validate security policies from files/env vars.

**Key Types**:
```rust
pub struct SecurityPolicy {
    pub version: String,
    pub enabled: bool,
    pub fail_mode: FailMode,
    // Module-specific configs...
}

pub struct PolicyBuilder {
    fn with_file(path) -> Result<Self>;
    fn with_env() -> Self;
    fn build() -> Result<SecurityPolicy>;
}

pub enum FailMode {
    Closed,   // Block threats
    Open,     // Log and allow
    LogOnly,  // Audit only
}
```

**Configuration Sources** (priority order):
1. Compiled defaults (secure)
2. Config file (`wg-bastion.toml`)
3. Environment variables (`WG_BASTION_*`)
4. Runtime overrides (graph/node-level)

**File Formats**: YAML, TOML, JSON

---

#### `pipeline` – Security Pipeline Framework

**Purpose**: Orchestrate multi-stage security checks with metadata propagation.

**Key Types**:
```rust
pub trait SecurityStage: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(&self, input: &str, ctx: &SecurityContext) 
        -> Result<StageResult>;
    fn should_run(&self, ctx: &SecurityContext) -> bool;
}

pub struct SecurityPipeline {
    pub async fn execute(&self, input: &str, ctx: &mut SecurityContext) 
        -> Result<()>;
}

pub struct SecurityContext {
    pub metadata: HashMap<String, String>,
    pub session_id: Option<String>,
    pub user_id: Option<String>,
}
```

**Stage Composition**:
```rust
let pipeline = SecurityPipeline::builder()
    .add_stage(InjectionScanner::new())
    .add_stage(PIIDetector::new())
    .add_stage(ModerationCheck::new())
    .build();
```

---

### Security Modules (Planned)

#### `prompt` – Prompt Protection

**Purpose**: Prevent system prompt extraction and leakage.

**Techniques**:
- **Fragmentation**: Split system prompts across multiple sections
- **Honeytokens**: Insert unique canaries to detect leakage
- **Role Boundaries**: Enforce system/user/assistant separation

**Key Types** (planned):
```rust
pub struct PromptGuard {
    fn fragment_prompt(&self, prompt: &str) -> Vec<String>;
    fn insert_honeytokens(&self, prompt: &str) -> (String, Vec<Honeytoken>);
}
```

---

#### `input` – Input Validation

**Purpose**: Scan user inputs for injection attacks, PII, and unsafe content.

**Stages**:
- **Injection Scanner**: Heuristic pattern matching
- **PII Detector**: Presidio integration or local regex
- **Moderation**: Content safety classification

**Key Types** (planned):
```rust
pub struct InjectionScanner {
    async fn scan(&self, input: &str) -> Result<InjectionResult>;
}

pub struct PIIDetector {
    async fn detect(&self, input: &str) -> Result<Vec<PIIMatch>>;
    fn redact(&self, input: &str, matches: &[PIIMatch]) -> String;
}
```

---

#### `output` – Output Validation

**Purpose**: Validate LLM outputs for safety, structure, and grounding.

**Stages**:
- **Schema Validator**: Enforce JSON Schema or custom DSL
- **Sanitizer**: Remove XSS, terminal escapes, dangerous code
- **Egress Scanner**: Detect secrets, PII, honeytokens
- **Grounding Validator**: Ensure responses match RAG context

**Key Types** (planned):
```rust
pub struct SchemaValidator {
    fn validate(&self, schema_name: &str, output: &Value) -> Result<()>;
}

pub struct EgressScanner {
    async fn scan(&self, output: &str) -> Result<EgressResult>;
}
```

---

#### `tools` – Tool & MCP Security

**Purpose**: Control tool execution and MCP protocol security.

**Features**:
- **Tool Policies**: YAML-based allowlists, risk scoring
- **Execution Guard**: Pre/post-execution hooks
- **MCP Security**: Session binding, token validation (per 2025-11-25 spec)
- **Approval Flow**: Human-in-the-loop for high-risk tools

**Key Types** (planned):
```rust
pub struct ToolPolicy {
    // Loaded from YAML
}

pub struct ExecutionGuard {
    async fn execute<F, T>(&self, tool: &str, f: F) -> Result<T>
        where F: Future<Output = T>;
}
```

---

#### `rag` – RAG Security

**Purpose**: Secure document ingestion and retrieval.

**Features**:
- **Sanitized Ingestion**: Validate URLs, sanitize HTML, scan for injection
- **Provenance**: Tag chunks with source, timestamp, trust level
- **Access Control**: Enforce tenant isolation in vector stores
- **Grounding**: Validate LLM outputs match retrieved context

---

#### `agents` – Agentic AI Security

**Purpose**: Control autonomous agent behavior.

**Features**:
- **Delegation Tracking**: Record agent-to-agent chains
- **Autonomy Boundaries**: Enforce action limits, budgets, kill switches
- **Memory Protection**: Validate agent state updates
- **Inter-Agent Security**: Authenticate agent communication

---

#### `abuse` – Abuse Prevention

**Purpose**: Prevent resource exhaustion and cost explosion.

**Features**:
- **Rate Limiting**: Multi-dimensional (user/session/tool/global)
- **Cost Monitoring**: Track token usage, enforce budgets
- **Recursion Guard**: Detect infinite loops
- **Circuit Breaker**: Protect external services

---

#### `telemetry` – Security Telemetry

**Purpose**: Structured events, audit logging, incident response.

**Features**:
- **Security Events**: Structured event schema
- **OTLP Export**: OpenTelemetry metrics and traces
- **Audit Logs**: Encrypted JSONL with retention
- **Incident Orchestrator**: Automated responses to threats

---

## Integration with weavegraph

### Hook Points (Planned API)

```rust
// GraphBuilder integration
let app = GraphBuilder::new()
    .with_security_policy(policy)  // Attach wg-bastion policy
    .build()?;

// Pre-node execution hook
app.on_pre_node(|ctx| async {
    input_pipeline.execute(&ctx.input, &mut ctx.security_ctx).await
});

// Post-node execution hook
app.on_post_node(|ctx| async {
    output_pipeline.execute(&ctx.output, &mut ctx.security_ctx).await
});

// EventBus integration for telemetry
app.event_bus().subscribe(|event| {
    security_sink.emit(SecurityEvent::from(event));
});
```

### NodeContext Extension

```rust
pub struct NodeContext {
    // Existing fields...
    pub security: SecurityContext,  // Injected by wg-bastion
}
```

---

## Data Flow

See [diagrams/data_flow.mmd](diagrams/data_flow.mmd) for the full Mermaid diagram.

**Simplified Flow**:
1. User input → Input Pipeline (injection/PII/moderation)
2. Validated input → Prompt Guard (fragmentation/honeytokens)
3. Protected prompt → weavegraph App Node (LLM call)
4. RAG query → RAG Security (ingestion/provenance)
5. Tool request → Tool Guard (policy/MCP validation)
6. LLM response → Output Pipeline (schema/sanitization/egress)
7. All stages → Telemetry Sink (events/audit/metrics)

---

## Configuration Management

### Example: `wg-bastion.toml`

```toml
version = "1.0"
enabled = true
fail_mode = "closed"

[input]
injection_detection = true
pii_detection = true
moderation = { enabled = false }  # Disable for performance

[output]
schema_validation = true
egress_scanning = true

[telemetry]
otlp_endpoint = "http://localhost:4317"
audit_retention_days = 365
```

### Environment Overrides

```bash
WG_BASTION_ENABLED=true
WG_BASTION_FAIL_MODE=open
WG_BASTION_INPUT_INJECTION_DETECTION=false
```

---

## Extension Points

### Custom Security Stages

Implement `SecurityStage` trait:

```rust
struct MyCustomStage;

#[async_trait]
impl SecurityStage for MyCustomStage {
    fn name(&self) -> &str { "my_custom_stage" }
    
    async fn execute(&self, input: &str, ctx: &SecurityContext) 
        -> Result<StageResult> 
    {
        // Custom validation logic
        if input.contains("forbidden") {
            return Ok(StageResult::fail("Forbidden content detected"));
        }
        Ok(StageResult::pass())
    }
}

// Add to pipeline
let pipeline = SecurityPipeline::builder()
    .add_stage(MyCustomStage)
    .build();
```

---

## Performance Considerations

### Latency Targets

| Configuration | P50 | P95 | P99 |
|---------------|-----|-----|-----|
| Heuristics only | <10ms | <50ms | <100ms |
| With local ML | <30ms | <100ms | <200ms |
| With remote APIs | <100ms | <200ms | <500ms |

### Optimization Strategies

1. **Parallel Stages**: Run independent checks concurrently
2. **Early Exit**: Stop pipeline on first failure (if configured)
3. **Caching**: Cache ML predictions for repeated inputs
4. **Streaming**: Process large payloads incrementally
5. **Feature Flags**: Disable expensive checks in dev/test

---

## Next Steps

This document will be expanded as implementation progresses:

- **Sprint 2-3**: Prompt protection module details
- **Sprint 4**: Input validation internals
- **Sprint 5-6**: Output validation and tool security
- **Sprint 7-8**: RAG and agent security
- **Sprint 9-10**: Abuse prevention and telemetry

---

*Last updated: January 2026 - Sprint 1 (WS1-03)*
