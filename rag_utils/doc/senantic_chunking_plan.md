# Semantic Chunking Scratch Plan

## Research Highlights
- **LangChain SemanticChunker**: Embedding-driven breakpoints (`percentile`, `stddev`, `interquartile`, `gradient`) paired with adjustable thresholds; emphasizes converting contiguous semantic units into `Document` objects with rich metadata (title, context windows, references) before embedding.
- **Pinecone RAG notebook**: Advocates for sentence-level preprocessing, contextual metadata (`prechunk`/`postchunk`, hierarchical IDs), and chunk post-processing tailored to the downstream consumer (retriever vs. LLM). Also demonstrates structuring chunking output as lightweight objects that can be further formatted for prompt construction.
- **Semantic Router**: Illustrates fast routing over embeddings, dynamic threshold optimization, and modular encoder abstraction (OpenAI, Cohere, HF). Reinforces keeping encoder selection pluggable, persisting computed vectors, and exposing hooks for future hybrid heuristics.
- **Industry best practices**: Normalize whitespace, strip boilerplate tags (`script`, `style`, `nav`, `aside`), carry provenance metadata (paths, DOM selectors), cache expensive embeddings, and design for deterministic fallbacks when embedding APIs fail.

## Module Scope & Goals
- Deliver an isolated `semantic_chunking` module with no live wiring yet, prepared for eventual integration with existing runtimes/event bus.
- Provide a general trait that captures shared semantics for chunkers, supporting configurable embedding providers, breakpoint strategies, and metadata capture.
- Ship two concrete implementations:
  - JSON-focused chunker that preserves key/value hierarchy and semantic coherence.
  - HTML-focused chunker that groups DOM content into meaningful blocks, strips noise, and handles mixed inline/block structures.
- Implement thorough tests and fixtures to validate segmentation, metadata, and deterministic behavior.

## Proposed Module Structure
```
src/
  semantic_chunking/
    mod.rs              // public exports + module docs
    config.rs           // shared config structs/enums
    types.rs            // SemanticChunk/Segment/Metadata definitions
    tokenizer.rs        // feature-gated token estimation helpers
    embeddings.rs       // Embedding provider trait + adapters (mock, OpenAI, etc.)
    breakpoints.rs      // breakpoint scoring strategies + detectors
    json.rs             // JSON chunker implementation
    html/
      mod.rs            // HTML chunker public API
      preprocess.rs     // DOM parsing/cleaning helpers
      grouper.rs        // tag grouping, block assembly
    tests.rs            // unit/integration tests (behind #[cfg(test)])
```
- Defer real embedding clients to follow-up (wire mock + trait now, leave TODO for concrete providers).
- Add `doc/semantic_chunking_plan.md` (this document) plus future design notes as needed.

## Shared Types & Trait Plan
1. **Define core data structures** (`types.rs`):
   - `SemanticChunk` with fields `id`, `content`, `tokens`, `metadata`, optional cached `embedding`, and context pointers (`prev_ids`, `next_ids`).
   - `ChunkMetadata` capturing source-specific details (e.g., JSON pointer, DOM path, tag summary, creation timestamps).
   - `ChunkingTrace` (optional) storing intermediate scores for debugging/telemetry.
   - `ChunkSegment`/`CandidateSegment` to represent pre-chunk atomic units before breakpoint detection.
2. **Config models** (`config.rs`):
   - `EmbeddingBackend` enum (e.g., `Mock`, `OpenAI`, `Cohere`, `LocalModel`) and related credentials placeholders.
   - `BreakpointStrategy` enum mirroring LangChain (`Percentile`, `StdDev`, `Interquartile`, `Gradient`) with tunable parameters.
   - `ChunkingConfig` struct: includes `max_tokens`, `min_tokens`, `similarity_threshold`, `overlap_config`, `metadata_flags`, `context_window`, `normalize_emoji`, etc.
   - `PreprocessConfig` for HTML-specific toggles (tags to drop/keep, attribute retention) and JSON-specific options (max depth, array flattening policies).
3. **Embedding Abstraction** (`embeddings.rs`):
   - Trait `EmbeddingProvider` with async `embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>` and sync helper for tests.
   - Implement `MockEmbeddingProvider` generating deterministic vectors (hash-based) for repeatable tests.
   - Outline (but stub) adapters for OpenAI/Cohere; maintain feature flags for future wiring.
4. **Breakpoint engine** (`breakpoints.rs`):
   - Create `BreakpointDetector` trait with method `fn detect(&self, scores: &[f32], config: &ChunkingConfig) -> Vec<usize>` returning indexes at which to split.
   - Implement strategies inspired by LangChain: compute pairwise cosine distance between adjacent embeddings, then apply detection algorithm:
     - Percentile: mark scores above configured percentile.
     - StdDev: `mean + n*std` threshold.
     - Interquartile: `Q3 + iqr_factor * (Q3 - Q1)`.
     - Gradient: derivative-based anomaly detection over score sequence; optionally apply Savitzky-Golay smoothing to reduce noise.
   - Provide helper to merge small chunks (ensure `>= min_tokens` by merging forward/backwards).
5. **Core trait** (`mod.rs`):
   - Define trait `SemanticChunker`:
     ```rust
     pub trait SemanticChunker {
         type Source;
         fn chunk(&self, source: Self::Source, cfg: &ChunkingConfig) -> Result<Vec<SemanticChunk>>;
     }
     ```
   - Provide default helper methods: `fn preprocess(...)`, `fn score_segments(...)`, `fn assemble_chunks(...)` to be optionally overridden.
   - Add `ChunkingOutcome` struct bundling `chunks`, `trace`, source stats (token counts, compression ratio).
   - Offer blanket implementation for `&T` where `T: SemanticChunker`, to ease usage.
6. **Telemetry hooks**: plan optional `EventBus` integration later; for now expose trace data and metrics counters (stub).

## JSON Chunker Implementation Plan (`json.rs`)
1. **Parsing & Validation**:
   - Accept inputs as `serde_json::Value`, `&str`, or `impl Read`; provide helper to parse string into Value with informative errors (line/column using `serde_json::Deserializer`).
   - Validate size constraints (max depth, total tokens) before processing; surface `ChunkingError::TooLarge` for guardrails.
2. **Segment Extraction**:
   - Traverse the JSON tree breadth-first to build `CandidateSegment` records containing:
     - `path`: JSON Pointer string (`/foo/0/name`).
     - `kind`: enum (`Object`, `Array`, `Value`) to drive heuristics.
     - `text`: human-friendly serialization (key + value summary) produced via configurable formatter.
     - `metadata`: e.g., depth, sibling index, hints about arrays vs. objects.
   - Flatten small scalar values into their parent object to avoid fragmenting short segments unless flagged as important (size threshold, keywords).
   - Optionally capture surrounding context by storing parent pointer for later `prechunk`/`postchunk` metadata.
3. **Normalization**:
   - Apply whitespace/emoji normalization, optionally convert numbers/dates to canonical forms.
   - For large text values (long strings), split into sentences using the `rust-bert` sentence segmenter when the optional feature is enabled, falling back to `unicode-segmentation` to avoid heavy deps in minimal builds; attach segments back to the same path for scoring.
4. **Embedding & Scoring**:
   - Batch segments into `Vec<String>` respecting provider limits (configurable `batch_size`).
   - Request embeddings via `EmbeddingProvider`; on failure, fall back to lexical heuristics (Jaccard on token bag) while logging via `ChunkingTrace`.
   - Compute cosine distances between sequential embeddings; optionally include hierarchical penalties (e.g., higher penalty when jumping across parent objects).
5. **Breakpoint Detection & Assembly**:
   - Feed distance series into `BreakpointDetector` chosen by config; adjust thresholds based on segment metadata (e.g., allow lower thresholds when switching top-level keys).
   - Post-process breakpoints to enforce `min_tokens`/`max_tokens`, merging segments as needed.
   - Construct `SemanticChunk` objects with aggregated content (join segment texts with newline), tokens estimated via existing tokenizer util (if available) or new `tiktoken_rs` wrapper.
6. **Metadata Enrichment**:
   - Include `source_path`, `keys`, `array_indices`, `parent_path`, `depth`, `segment_range` (start/end indices) in `ChunkMetadata`.
   - Provide optional context pointers: previous/next chunk IDs for quick retrieval of neighbors, plus `crumb_trail` showing hierarchical breadcrumbs.
7. **Testing**:
   - Add fixtures under `tests/fixtures/json/` (e.g., nested config, product catalog, long text arrays).
   - Write unit tests covering: deterministic chunk IDs, respect for `max_tokens`, threshold behavior, fallback path (mock embedding failure), and metadata correctness.
   - Include property-style test ensuring full reconstruction: concatenating chunk contents equals normalized original text.

## HTML Chunker Implementation Plan (`html/mod.rs` & submodules)
1. **Parsing Infrastructure**:
   - Use the `scraper` crate (html5ever-backed) for DOM traversal and CSS-style selection; keep `lol_html` on the backlog for streaming requirements.
   - Normalize input encoding (UTF-8) and strip BOM; handle `<meta charset>` detection if needed.
2. **Cleaning & Preprocessing** (`preprocess.rs`):
   - Remove irrelevant nodes: `script`, `style`, `noscript`, `iframe`, `svg`, `nav`, `footer`, `aside` (configurable allowlist/denylist).
   - Collapse whitespace, decode HTML entities, drop comments.
   - Optionally capture `<title>`, `<meta name="description">` for metadata enrichment.
3. **Structure-Aware Grouping** (`grouper.rs`):
   - Identify block-level containers (`article`, `section`, `div`, `p`, `ul/ol/li`, headings, tables).
   - Combine inline nodes under same block; merge adjacent paragraphs under same heading until thresholds satisfied.
   - For headings (`h1-h6`), start a new segment and tag subsequent blocks until next heading (facilitates topic grouping similar to Pinecone example's `title` context).
   - Handle lists and tables specially: convert to markdown-like text for readability while preserving structure in metadata (e.g., list depth, table headers).
4. **Segment Normalization**:
   - Apply readability heuristics (e.g., skip nav duplicates, drop empty text, enforce min character counts).
   - Set up optional summarization of attributes (id/class) to keep CSS hooks minimal but traceable.
   - Sentence-split large text blocks using the feature-gated `rust-bert` segmenter (fallback to lightweight splitter) while tracking the originating DOM path.
5. **Embedding & Breakpoints**:
   - Batch text segments for embeddings; incorporate heading context (prepend heading path to each block akin to Pinecone approach).
   - Compute pairwise cosine distances; mix in structural signals such as heading level jumps or DOM depth changes (linearly combine with embedding distance before detection).
   - Expose HTML-specific adjustments: e.g., treat large `div` transitions as stronger breakpoints, enforce chunk boundaries at heading changes regardless of embedding score if chunk would exceed `max_tokens`.
6. **Chunk Assembly**:
   - When building `SemanticChunk`, join block texts separated by blank lines; maintain `dom_path` (CSS selector style), `heading_hierarchy`, `lang` attribute, and `source_url` if provided via config.
   - Populate context metadata: preceding/following headings, sibling index, and snippet of raw HTML for debug (hidden behind config flag).
7. **Testing**:
   - Fixtures covering: blog article, documentation page with nested headings, HTML with heavy boilerplate, malformed HTML for robustness.
   - Unit tests verifying tag stripping, heading grouping, fallback when embeddings disabled, and deterministic outputs.
   - Golden tests comparing chunk boundaries against expected indices; fuzz test with randomized DOM to ensure no panic.

## Cross-Cutting Concerns & Utilities
- **Tokenizer utilities**: integrate `tiktoken-rs` for accurate token estimation with a compile-time fallback to whitespace counts when the optional feature is disabled.
- **Caching strategy**: optional in-memory cache keyed by hash of text + backend ID to avoid recomputing embeddings during repeated runs/tests.
- **Concurrency**: chunk long documents using tokio async tasks; guard with config to keep deterministic order.
- **Error handling**: define `ChunkingError` enum (invalid input, embedding failure, threshold failure) with `thiserror` for readability.
- **Logging/telemetry**: integrate with existing logging facade (likely `tracing`), including TRACE-level span around embedding calls and breakpoint detection.
Also integrate with `EventBus` for streaming chunking progress.

## Testing & Validation Roadmap
1. Unit tests per chunker module with mock embeddings.
2. Integration tests exercising full pipeline from raw input string -> `SemanticChunk` list.
3. Property-based tests verifying reconstruction, monotonic chunk ordering, and stability across repeated runs.
4. Benchmark harness (optional) measuring throughput on large HTML/JSON to prepare for future production tuning.

## Future Integration Notes
- Expose module via `SemanticChunkingService` later to plug into existing runtimes (likely via `src/runtimes`), using event bus to request chunking jobs.
- Provide CLI demo in `src/run_demoX.rs` once implementations stabilize.
- Explore extending breakpoints to support hybrid lexical-semantic scoring and user-defined guardrails (e.g., force split on JSON schema boundaries).
-

## Runtime Integration Notes (2025-10-01T17:05:00Z)
- Service layer responsibilities: config layering, embedder resolution (RIG -> `EmbeddingProvider` adapter), document kind detection, and centralised telemetry emission.
- Builder should accept defaults from `SemanticChunkingModuleConfig`, optional `Arc<dyn EmbeddingModelDyn>`, and cache configuration controls (capacity toggles, shared handles where needed).
- Request API will allow overrides per call (chunking config, preprocess knobs, embedder) and accept either raw payloads or filesystem paths; file resolution uses async I/O and extension-based dispatch.
- Telemetry instrumentation: wrap chunking in `tracing::span!("semantic_chunk").in_scope`, record counters (cache hits/misses, fallback_used, smoothing_window) and thread them into the returned `ChunkTelemetry` payload; graft remains responsible for streaming these details over its event bus.
- Response returns `ChunkingOutcome` plus the derived telemetry snapshot, exposing the `Vec<SemanticChunk>` for callers to mutate or enrich before persistence.

## SemanticChunkingService TODOs (2025-10-01T17:05:00Z)
- Implement `RigEmbeddingProvider` adapter bridging `rig::embeddings::EmbeddingModel` into existing `EmbeddingProvider` trait (convert `Vec<f64>` to `Vec<f32>`, respect max batch size).
- Introduce `service.rs` under `semantic_chunking` with builder, request/response structs, telemetry tracking, and dispatch logic to JSON/HTML chunkers.
  - Builder: surface module defaults, optional shared caches (capacity, shared handle reuse), and allow callers to inject embedder handles (RIG or custom `EmbeddingProvider`).
  - Request API: enum to cover inline JSON/HTML/plain text and filesystem paths, with per-call overrides for chunking/preprocess configs and embedder selection.
  - Execution path: resolve source -> choose chunker -> execute under tracing span -> capture cache metrics + fallback flag -> populate the telemetry struct before returning.
  - Response: wrap `ChunkingOutcome` alongside a `ChunkTelemetry` struct (durations, cache hit/miss counters, smoothing window, embedder label) so runtimes can forward metrics without recomputing; no direct event bus dependency lives in `rag_utils`.
- Add tracing spans for cache hits, lexical fallback, and smoothing to align with runtime observability roadmap; surface the counters via the returned telemetry data.
- Unit tests covering: JSON string input, HTML file path, lexical fallback when embedder missing, RIG embedding adapter (use minimal fake EmbeddingModel), telemetry snapshots, and config overrides.

## Rust Book Scraper & Vector Store Ingestion (2025-10-01T18:05:00Z)
- Build a scraping orchestrator that ingests the full Rust Book and persists chunk embeddings into `rig-sqlite`:
  1. **Discovery**: fetch the book TOC (index page) and extract chapter URLs using `reqwest` + `scraper` (already in deps); respect robots.txt and throttle requests.
  2. **Download**: retrieve each chapter asynchronously (bounded concurrency/semaphore) with retry/backoff and a custom User-Agent.
  3. **Chunk**: call `SemanticChunkingService::chunk_document(ChunkSource::Html(page_html))` per chapter using a configured RIG embedding model; capture telemetry to track fallback/cache behaviour.
  4. **Persist**: upsert each chunk into a `rig_sqlite::VectorStore`, storing `SemanticChunk` content, metadata (URL, heading chain, segment positions), and embeddings (convert `Vec<f32>` back to `Vec<f64>` or adjust provider to skip downcast).
  5. **Audit**: use graft’s event bus to log progress (per-page diagnostic and aggregate stats), and optionally serialize `ChunkTelemetry` snapshots for monitoring dashboards.
- CLI entrypoint `cargo run --example ingest_rust_book`:
  - Flags: `--base-url`, `--out-db`, `--embedder <provider>` (OpenAI/Cohere/Ollama), `--concurrency`, `--resume`.
  - Emits telemetry to stdout and writes checkpoint file (list of completed URLs) so reruns skip finished chapters.
- Testing strategy:
  - Use a mock HTTP server (e.g., `wiremock-rs`) serving two chapter fixtures to validate scraper + chunker + vector-store pipeline.
  - Unit test ensures metadata (URL, heading hierarchy, chunk ids) survives round-trip into SQLite vector store.
- Best practices:
  - Cache downloaded HTML under `.graft/cache/rust-book/` to avoid re-fetching during development.
  - Persist chunk `id`/`prev_ids`/`next_ids` to support navigation when reconstructing context windows.
  - Wrap each page ingestion in a tracing span (`tracing::info_span!("rust_book_chapter", url)`) and propagate telemetry counters to the event bus.

### Run Demo5 Implementation Plan (2025-10-01T18:12:00Z)
1. **Module scaffolding**
   - `src/rag/mod.rs` re-export helpers; add `rag::ingest::mod` with `rust_book.rs` implementing scraping + chunking orchestrator.
   - Introduce `rag::store::sqlite.rs` thin wrapper over `rig_sqlite::VectorStore` (open/connect, upsert chunks, basic retrieval helper).
   - Shared types: `IngestStats`, `StoredChunk` capturing DB row metadata.
2. **Scraper implementation**
   - Use `reqwest` + `tokio` for async fetch; parse TOC via `scraper::Html` extracting chapter links under `nav#TOC`.
   - Respect concurrency limiter via `tokio::sync::Semaphore`; include exponential backoff + retry for transient 5xx.
   - Write optional HTML cache file per URL (hashed path) gated behind `IngestOptions::cache_dir`.
3. **Chunking pipeline**
   - Reuse `SemanticChunkingService`; build once with desired embedder and cache configuration.
   - For each HTML page, call `chunk_document`, convert embeddings to `Vec<f64>` needed by `rig_sqlite`, collect telemetry for logging/streaming via graft components.
4. **Vector store integration**
   - Connect to SQLite DB (create if missing); upsert chunks via `vector_store.upsert(&id, embedding, metadata_json)`.
   - Maintain mapping table for chapter URL -> chunk ids to support deletion/resume; simple table using `rusqlite` or store in metadata.
5. **Example entrypoint (`run_demo5.rs`)**
   - CLI args via `clap` (feature optional) or manual parsing: base URL, DB path, concurrency, resume flag, query prompt.
   - Ingest pipeline streams diagnostics to stdout via graft’s event bus while populating DB.
   - After ingest, build `VectorStoreRetriever` and run a sample query (embedding the question) to print top hits with snippet context.
6. **Testing**
   - Unit: mock HTTP server (two chapter fixtures) verifying that ingest writes expected rows and retrieval returns chunk from fixture.
   - Integration: run `run_demo5` against cached fixtures in CI (feature gated) to avoid hitting network.
7. **Documentation & telemetry**
   - Update README or demo docs referencing `run_demo5` usage.
   - Add progress log entries as implementation proceeds; capture telemetry counters in ingest summary.

Progress Log Addendum:
- 2025-10-01T18:12:47: Scoped run_demo5 ingestion pipeline (scraper + chunker + vector store) and planned module layout/tests for end-to-end example.

## Progress Log
- 2025-10-01T10:02:13: Initial review of module plan; validated structure, flagged dependency strategy (tiktoken-rs for tokens, scraper for DOM, optional rust-bert feature for sentence splitting) and noted need to revisit breakpoint heuristics once richer signals available.
- 2025-10-01T10:03:52: Added crate dependencies (scraper, unicode-segmentation, optional tiktoken-rs + rust-bert features) and documented feature-gating strategy to keep default builds light while enabling richer tokenization when desired.
- 2025-10-01T10:04:28: Starting tokenizer integration (feature-gated tiktoken fallback) before chunker refinements.
- 2025-10-01T10:05:13: Implemented `tokenizer.rs` with feature-gated tiktoken encoder (cached via `OnceLock`) and whitespace fallback; wired `estimate_tokens` to new helper, noting future need for model selection + caching heuristics.
- 2025-10-01T10:05:37: Ran `cargo fmt` + `cargo check` to confirm new tokenizer wiring builds cleanly.
- 2025-10-01T10:28:58: Current module snapshot — scaffolding in place (config, types, trait, mock embedding provider, breakpoint heuristics, HTML/JSON chunker stubs, tokenizer integration). JSON chunker currently serializes entire tree; HTML chunker uses sanitized single-block grouping; tests cover smoke scenarios with mock embeddings. Added feature-gated token counting (`tokenizer.rs`) and optional dependencies (tiktoken-rs, rust-bert) with default enabling for tokenizer accuracy; introduced `scraper`, `unicode-segmentation` as groundwork for upcoming parsing improvements.
- 2025-10-01T10:31:25: Begin JSON chunker implementation — planning traversal/segmentation strategy (object aggregation, scalar folding, sentence splitting heuristic) and embedding fallback design before coding.
- 2025-10-01T10:35:13: Implemented JSON traversal pipeline: object/array handlers aggregate scalars, emit string segments with heuristic sentence batching (target ~160 tokens), record segment metadata (paths, positions, kinds). Introduced fallback lexical scoring via Jaccard distance when embeddings fail and average chunk embeddings when available.
- 2025-10-01T10:35:13: Added chunk assembly pass enforcing min-token merges, collecting trace events (distance, breakpoints, fallback flags), and populating metadata/prev-next ids. Updated unit test to assert non-empty chunk list and metadata/trace presence; ran `cargo fmt` + `cargo check` to validate.
- 2025-10-01T11:09:02: Initiated HTML chunker overhaul — planning DOM sanitization with `scraper`, heading-aware grouping, block merging heuristics, and parity with JSON breakpoint assembly before coding.
- 2025-10-01T11:19:07: Implemented HTML preprocessing with `scraper`: parses DOM, skips drop tags, tracks heading stack, builds `BlockCandidate` records (paths, depth, position) with sentence-preserving text extraction and whitespace normalization heuristics.
- 2025-10-01T11:19:07: Added `HtmlBlock` grouping + full HTML chunker pipeline: heading-aware merging, metadata-rich segments, embedding fallback, breakpoint assembly mirroring JSON, and tracing instrumentation; updated token + metadata propagation across segments/chunks.
- 2025-10-01T11:34:39: Continuing follow-up work — extracting shared chunk assembly utilities and enhancing HTML tests per plan; considering optional sentence-segmentation feature once scaffolding stabilizes.
- 2025-10-01T11:43:17: Extracted shared chunk assembly helpers (`assembly.rs`) to centralize breakpoint planning, trace construction, neighbor linking, and stats; rewired JSON/HTML chunkers to reuse them and trimmed duplicated utilities.
- 2025-10-01T11:43:17: Added heading-aware HTML test coverage and ensured `SemanticChunker` trait usage via shared helper imports; `cargo test html_chunker_captures_heading_metadata` now passes.
- 2025-10-01T12:24:10: Starting next work block — focus on fixture-driven tests (JSON & HTML), optional rust-bert sentence splitting integration, and caching/telemetry hooks per roadmap.
- 2025-10-01T12:36:48: Added fixture-backed JSON/HTML tests (including caching behavior via counting provider) to validate chunk outputs, metadata enrichment, and cache reuse.
- 2025-10-01T12:36:48: Introduced configurable embedding cache (shared assembly helpers now reused by both chunkers) and sentence splitter module with improved regex heuristic; documented deferral of heavier rust-bert integration after evaluating available pipelines.
- 2025-10-01T13:32:55: Wired cache configuration into chunkers via `ChunkingConfig::cache_capacity`, added runtime cache initialization, and exposed `with_cache_capacity/without_cache` builders; caching now persists across runs without manual setup.
- 2025-10-01T13:32:55: Introduced regex-based sentence splitting module with feature-gate hook point for future advanced tokenizers; JSON segmentation now reuses it while keeping the fallback extensible.
- 2025-10-01T13:32:55: Added fixture-driven golden tests for JSON/HTML breakpoints plus cache reuse verification; embedded fixtures under `tests/semantic_chunking/` and expanded the counting provider to track embedding invocations.
- 2025-10-01T14:24:37: Added optional `segtok`-powered sentence splitter (feature `semantic-chunking-segtok`) with graceful fallback to regex heuristics, improving sentence boundaries without large model dependencies.
- 2025-10-01T14:24:37: Introduced additional fixtures (HTML table, JSON metrics) plus lexical-fallback tests to verify breakpoint stability and error recovery; added failing embedding provider to assert fallback traces.
- 2025-10-01T14:24:37: Connected cache settings to `ChunkingConfig::cache_capacity`, standardized per-chunk `configure_cache`, and ensured service builders honour runtime toggles while allowing caches to be disabled via `capacity=0`.
- 2025-10-01T14:47:07: Added configurable sentence splitter enum (default regex, optional segtok via feature), runtime cache toggles via `ChunkingConfig`, and broadened fixture coverage with fallback tests for lexical mode.
- 2025-10-01T15:04:53: Beginning work on enhanced testing (fixtures/property checks) and breakpoint/segmentation refinements (structural weighting, score smoothing).
- 2025-10-01T15:14:11: Added structural breakpoint weighting (depth/path/heading cues) and score smoothing to the assembly pipeline; both chunkers now blend semantic/structural signals based on config knobs.
- 2025-10-01T15:14:11: Expanded test matrix with malformed/structured fixtures and lexical fallback checks; verified cache reuse and segmentation behaviour via new counting/failing providers.
- 2025-10-01T16:47:17: Re-read LangGraph port, event bus, and error handling docs to anchor runtime/telemetry integration scope; outlined initial needs for service API, config propagation, and structured traces.
- 2025-10-01T16:55:42: Surveyed semantic chunking modules, runtime runner, telemetry/event bus plumbing; sketched service orchestration points (embedder registry, cache toggles, tracing spans, bus diagnostics) in integration notes section.
- 2025-10-01T17:44:46: Implemented `SemanticChunkingService` (JSON/HTML dispatch, RIG adapter, cache metrics, tracing span + event bus emission) with unit coverage for embeddings, caching reuse, file ingestion, and diagnostics.
- 2025-10-01T18:35:12: Added `rag` module scaffolding (SQLite chunk store + Rust Book ingestor), hooked service + vector store wiring, and drafted run_demo5 pipeline for scraping, chunking, and retrieval.
- 2025-10-01T19:12:05: Refactored `run_demo5` into multi-node graph (scrape → chunk → store → retrieve) with ctx.emit telemetry, filesystem handoff between nodes, and SQLite-backed retriever answer flow.
- 2025-10-01T16:32:21Z: Kicking off follow-up iteration — planning concurrency/backoff for scraping, fixture-driven ingest tests, and richer metadata persistence for retrieval responses.
- 2025-10-01T17:00:06Z: Added bounded concurrency + retry/backoff to scraping node, introduced mock-server ingestion test harness with custom embedding model, and enriched retrieval to persist structured match metadata with vector-search fallback.
- 2025-10-01T19:51:38Z: Instrumented `SemanticChunkingService` to emit per-run diagnostics (stats, chunk previews, breakpoint traces) over the shared event bus so node progress logs surface grouping rationale without extra listeners.
- 2025-10-01T20:39:26Z: Ensured `run_demo5` loads `.env` via `dotenvy::dotenv()` before initializing Gemini client so environment-backed embedding configuration matches demo4 behavior.
