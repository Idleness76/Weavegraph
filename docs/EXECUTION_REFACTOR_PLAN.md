# Execution Refactor Plan

This document outlines purposeful, idiomatic refactors and enhancements for the workflow runtime (`AppRunner`) focusing on execution flow (`run_step`, `run_one_superstep`, `run_until_complete`), state handling, instrumentation, error propagation, and pause/resume extensibility. Changes are staged to preserve stability and avoid fragmentation.

## Scope
- Refactor large functions only where logical phase boundaries improve clarity and testability.
- Reduce unnecessary cloning of `SessionState` without introducing borrow complexity.
- Enhance instrumentation using `tracing` spans where they add diagnostic value.
- Expand error propagation only with actionable context.
- Introduce pause/resume hook structure aligned with future control commands.

## Current Structure Summary (Relevant Parts)
- `run_step`: clones `SessionState`, performs interrupt checks, delegates to `run_one_superstep`, persists state, handles pause conditions.
- `run_one_superstep`: combines (1) scheduling, (2) barrier application, (3) frontier computation, (4) completion determination.
- `run_until_complete`: loop with termination check, per-step execution, final state extraction and logging.
- Error events embed context but error variants do not carry full frontier or version metadata.

## Refactor Targets
### 1. `run_one_superstep`
Split into three private helpers to isolate semantics:
1. `fn schedule_step(&self, session_state: &mut SessionState, step: u64) -> Result<SchedulerOutcome, RunnerError>`
   - Wrap existing scheduler invocation + output normalization (ordering partials).
2. `async fn apply_barrier_and_update(&self, session_state: &mut SessionState, ran: &[NodeKind], partials: Vec<NodePartial>) -> Result<BarrierOutcome, RunnerError>`
   - Encapsulate cloning state, barrier invocation, and assignment.
3. `fn compute_next_frontier(&self, session_state: &SessionState, ran: &[NodeKind], barrier: &BarrierOutcome) -> Vec<NodeKind>`
   - Frontier command resolution + conditional edge evaluation.

`run_one_superstep` then orchestrates these phases and constructs `StepReport`. This separation clarifies failure points and allows targeted instrumentation.

Helper skeletons (exact types aligned with current code):
```rust
// Outcome from scheduler after normalization (ordered partials)
struct SchedulerOutcome {
    ran_nodes: Vec<NodeKind>,
    skipped_nodes: Vec<NodeKind>,
    partials: Vec<NodePartial>,
}

// NOTE: schedule_step must be async (not sync as initially suggested)
// because scheduler.superstep() requires .await in async context
#[inline]
async fn schedule_step(
    &self,
    session_state: &mut SessionState,
    step: u64,
) -> Result<SchedulerOutcome, RunnerError> {
    let snapshot = session_state.state.snapshot();
    let result = session_state.scheduler.superstep(
        &mut session_state.scheduler_state,
        self.app.nodes(),
        session_state.frontier.clone(),
        snapshot.clone(),
        step,
        self.event_bus.get_emitter(),
    ).await?;

    let mut by_kind: rustc_hash::FxHashMap<NodeKind, NodePartial> = rustc_hash::FxHashMap::default();
    for (k, p) in result.outputs { by_kind.insert(k, p); }
    let ran = result.ran_nodes.clone();
    let partials = ran.iter().cloned().filter_map(|k| by_kind.remove(&k)).collect();
    Ok(SchedulerOutcome { ran_nodes: ran, skipped_nodes: result.skipped_nodes, partials })
}

#[tracing::instrument(skip(self, session_state, partials, ran), err)]
async fn apply_barrier_and_update(
    &self,
    session_state: &mut SessionState,
    ran: &[NodeKind],
    partials: Vec<NodePartial>,
) -> Result<BarrierOutcome, RunnerError> {
    let mut update_state = session_state.state.clone();
    let outcome = self
        .app
        .apply_barrier(&mut update_state, ran, partials)
        .await
        .map_err(RunnerError::AppBarrier)?;
    session_state.state = update_state;
    Ok(outcome)
}

#[inline]
fn compute_next_frontier(
    &self,
    session_state: &SessionState,
    ran: &[NodeKind],
    barrier: &BarrierOutcome,
) -> Vec<NodeKind> {
    // Implementation mirrors current logic: resolve commands, then conditional edges when not replaced
    // Keep order stability and deduplicate targets.
    // Intentionally left as a direct port to avoid semantic drift.
    let mut next: Vec<NodeKind> = Vec::new();
    // ...
    next
}
```

### 2. `run_until_complete`
Extract two helpers:
- `fn is_session_complete(&self, s: &SessionState) -> bool` (current frontier termination logic).
- `fn finalize_state_snapshot(&self, session_id: &str) -> Result<(VersionedState, StateVersions, u64), RunnerError>` returning final cloned state, versions, and last step. Logging occurs after retrieval.

Benefits: Easier unit tests for termination logic and finalization independently.

### 3. `run_step` Clone Reduction
Current pattern:
```rust
let mut session_state = self.sessions.get(session_id)?.clone();
```
Proposed:
- Borrow mutably via `if let Some(state) = self.sessions.get_mut(session_id)` eliminating full struct clone.
- Maintain temporary data (e.g. frontier snapshot) before async calls to avoid borrow checker issues.
- If borrow conflicts arise with async `.await` (scheduler and barrier), scope mutable borrows locally: extract needed fields (frontier, scheduler refs) into locals, release borrow, then re-borrow mutably for state update.

If borrow restructuring becomes overly complex, alternative: Wrap `SessionState` in `Arc<Mutex<_>>` was considered but rejected to avoid synchronization overhead and non-idiomatic complexity. Prefer lifetime-scoped mutable borrows.

Concrete re-borrow pattern:
```rust
#[tracing::instrument(skip(self, options), err)]
pub async fn run_step(
    &mut self,
    session_id: &str,
    options: StepOptions,
) -> Result<StepResult, RunnerError> {
    // Phase 1: take minimal snapshots under a short-lived mutable borrow
    let (step_before, frontier, messages_version, extra_version);
    {
        let s = self.sessions.get_mut(session_id)
            .ok_or_else(|| RunnerError::SessionNotFound { session_id: session_id.to_string() })?;
        step_before = s.step;
        frontier = s.frontier.clone(); // small clone vs cloning whole SessionState
        messages_version = s.state.messages.version();
        extra_version = s.state.extra.version();
    }

    // Interrupts: use the snapshot without holding the borrow
    if frontier.is_empty() || frontier.iter().all(|n| *n == NodeKind::End) {
        return Ok(StepResult::Completed(StepReport {
            step: step_before,
            ran_nodes: vec![],
            skipped_nodes: frontier,
            barrier_outcome: BarrierOutcome::default(),
            next_frontier: vec![],
            state_versions: StateVersions { messages_version, extra_version },
            completed: true,
        }));
    }

    if options.interrupt_before.iter().any(|n| frontier.contains(n)) {
        // Re-borrow to build PausedReport from the live session
        let s = self.sessions.get(session_id).unwrap();
        return Ok(StepResult::Paused(PausedReport { session_state: s.clone(), reason: PausedReason::BeforeNode(options.interrupt_before[0].clone()) }));
    }

    // Phase 2: re-borrow mutably for the superstep and updates
    let step_result = {
        let s = self.sessions.get_mut(session_id).unwrap();
        self.run_one_superstep(s).await
    }?;

    // Phase 3: persist and post-interrupt checks
    {
        let s = self.sessions.get(session_id).unwrap();
        if self.autosave {
            if let Some(cp) = &self.checkpointer { let _ = cp.save(Checkpoint::from_session(session_id, s)).await; }
        }
    }

    // After-node and per-step pauses can be handled using step_result
    // ...
    Ok(StepResult::Completed(step_result))
}
```

### 4. Checkpoint Save Consolidation
Introduce:
```rust
async fn maybe_checkpoint(&self, session_id: &str, session_state: &SessionState)
```
Centralizes autosave pattern and makes instrumentation (`#[instrument]` or span) uniform.

Suggested helper:
```rust
#[tracing::instrument(skip(self, session_state))]
async fn maybe_checkpoint(&self, session_id: &str, session_state: &SessionState) {
    if self.autosave {
        if let Some(cp) = &self.checkpointer {
            let _ = cp.save(Checkpoint::from_session(session_id, session_state)).await;
        }
    }
}
```

### 5. Instrumentation Enhancements
Keep existing top-level `#[instrument]` on public execution methods. Add spans at phase boundaries where latency attribution is useful:
- `schedule_step`: span name `"schedule"` fields: `step`, `frontier_len`.
- `apply_barrier_and_update`: span `"barrier"` fields: `ran_nodes_len`, `errors_in_partials`.
- `compute_next_frontier`: span `"frontier"` fields: `commands_count`, `conditional_edges_evaluated`.
- `maybe_checkpoint`: span `"checkpoint"` field: `step`.
Avoid adding spans inside tight loops (e.g. per conditional edge) unless profiling indicates need; retain `debug!` events for per-edge routing.

Span usage examples:
```rust
#[tracing::instrument(skip(self, session_state), fields(step = session_state.step + 1), err)]
async fn run_one_superstep(&self, session_state: &mut SessionState) -> Result<StepReport, RunnerError> {
    // schedule
    let schedule_span = tracing::info_span!("schedule", frontier_len = session_state.frontier.len());
    let outcome = schedule_span.in_scope(|| {
        // call schedule_step() (sync helper) or scheduler.superstep().await
    });

    // barrier
    let barrier_span = tracing::info_span!("barrier", ran_nodes_len = outcome.ran_nodes.len());
    // apply_barrier_and_update(...).await

    // frontier
    let frontier_span = tracing::info_span!("frontier");
    // compute_next_frontier(...)
    // ...
}
```

### 6. Error Propagation Improvements
Augment `RunnerError::Scheduler` variant (or introduce new structured error) to include:
- `frontier_snapshot: Vec<NodeKind>` (at failure time)
- `state_versions: StateVersions`
- Optional `node_kind` (already present for `NodeRun`)
Benefits: Upstream callers can decide retry/resume strategies. Provide constructor helper to avoid verbosity.

Example (conceptual):
```rust
pub enum RunnerError {
    Scheduler(SchedulerError, FrontierContext),
    // ... existing variants
}

pub struct FrontierContext {
    pub frontier: Vec<NodeKind>,
    pub versions: StateVersions,
}
```
Only populate for scheduling or barrier failures; skip for session-not-found.

Concrete error shaping with `thiserror` and `miette`:
```rust
#[derive(Debug, thiserror::Error, miette::Diagnostic)]
pub enum RunnerError {
    #[error(transparent)]
    #[diagnostic(code(wg.runner.checkpointer))]
    Checkpointer(#[from] CheckpointerError),

    #[error("scheduler error")]
    #[diagnostic(code(wg.runner.scheduler))]
    Scheduler {
        #[source]
        source: SchedulerError,
        #[help]
        context: FrontierContext,
    },

    // existing variants ...
}

#[derive(Debug, Clone)]
pub struct FrontierContext {
    pub frontier: Vec<NodeKind>,
    pub versions: StateVersions,
}

impl FrontierContext {
    pub fn capture(s: &SessionState) -> Self {
        Self {
            frontier: s.frontier.clone(),
            versions: StateVersions {
                messages_version: s.state.messages.version(),
                extra_version: s.state.extra.version(),
            },
        }
    }
}
```

### 7. Pause/Resume Hooks
Add lightweight trait-based hooks for future control-plane integration:
```rust
pub trait RunHooks: Send + Sync {
    fn before_step(&self, session_id: &str, step: u64, frontier: &[NodeKind]) {}
    fn after_step(&self, session_id: &str, report: &StepReport) {}
    fn on_pause(&self, session_id: &str, paused: &PausedReport) {}
}
```
- `AppRunner` holds `hooks: Vec<Arc<dyn RunHooks>>`.
- Invoke in `run_step` after interrupt checks and after step completion.
Provides extension without coupling to external control logic. Tests can inject a test hook to assert pause behaviors.

Registration and invocation:
```rust
pub struct AppRunner {
    // ...
    hooks: Vec<std::sync::Arc<dyn RunHooks>>, 
}

impl AppRunner {
    pub fn add_hook(&mut self, hook: std::sync::Arc<dyn RunHooks>) { self.hooks.push(hook); }
}

// In run_step:
for h in &self.hooks { h.before_step(session_id, step_before + 1, &frontier); }
// After step completion or pause:
match &result {
    StepResult::Completed(r) => for h in &self.hooks { h.after_step(session_id, r); },
    StepResult::Paused(p) => for h in &self.hooks { h.on_pause(session_id, p); },
}
```

### 8. Testing Strategy
Add focused unit tests:
- `compute_next_frontier` (conditional edges + replace/append precedence).
- Clone removal: assert memory address equality of unchanged scheduler across steps (indirectly via debug counters or instrumentation hooks).
- Hooks: verify `on_pause` triggers when interrupts configured.
- Error context: simulate node failure and assert enriched `RunnerError` fields.

Property tests (optional future): frontier resolution determinism under mixed command ordering.

Test skeletons:
```rust
#[tokio::test]
async fn compute_next_frontier_respects_replace_before_append() {
    // build small app with Start -> A, commands issuing Replace then Append
    // assert next frontier contains only Replace entries
}

#[tokio::test]
async fn run_step_avoids_full_session_clone() {
    // instrument AppRunner to count clones (via a custom type inside SessionState or hook)
    // ensure single borrow flow works and final state updates are correct
}

#[tokio::test]
async fn hooks_on_pause_are_called() {
    // register a test hook capturing calls; configure interrupt_each_step
    // assert before_step and on_pause were called
}

#[tokio::test]
async fn scheduler_error_carries_frontier_context() {
    // force a node failure; match RunnerError::Scheduler and assert context.frontier non-empty
}
```

### 9. Performance Considerations
- Reducing full `SessionState` clone per step lowers allocation churn when state grows (messages/extra channels). Benchmark with existing `benches/event_bus_throughput.rs` pattern extended for step throughput.
- Additional spans introduce minor overhead; keep them coarse-grained to avoid hot-loop penalties.

### 10. Migration Approach (Staged)
1. Introduce helper functions + tests without removing original bodies (feature branch). Gate with `cfg(feature="refactor_phases")` if needed.
2. Refactor `run_step` borrow logic; run full test suite and compare allocations using `cargo +nightly bench` + `--profile release`.
3. Add checkpoint helper and instrumentation spans; validate logs.
4. Implement error context struct and adapt error handling paths; update tests expecting variants.
5. Add hooks system and tests; mark experimental in docs.
6. Remove deprecated internal code paths; update README usage notes if public API changed.

Feature flag example:
```rust
// Cargo.toml
[features]
refactor_phases = []

// In code
#[cfg(feature = "refactor_phases")]
fn schedule_step(...) { /* new helper */ }
```

### 11. API Stability Notes
- Keep public method signatures unchanged initially (`run_step`, `run_one_superstep`, `run_until_complete`).
- New helpers private; only expose hooks registration method (`add_hook`).
- Document error variant change in `CHANGELOG`.

## Risks and Mitigations
- Borrow checker refactor complexity: mitigate by incremental helper extraction, using temporary local snapshots.
- Over-instrumentation: keep span count limited to 4 new spans; rely on existing `debug!` for granular routing.
- Error variant expansion: downstream pattern matches may break; version bump and clear release notes.

## Summary
Refactors focus on clarity (phase separation), efficiency (clone removal), observability (targeted spans), richer debugging (frontier/version context in errors), and extensibility (hooks). Each change is bounded to avoid gratuitous fragmentation and aligns with enterprise Rust practices.

## Next Steps (After Approval)
Proceed with staged implementation per section 10.
