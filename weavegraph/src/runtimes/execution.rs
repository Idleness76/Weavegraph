//! Step execution types and logic for workflow runs.
//!
//! This module defines the types used to represent step execution results,
//! pause conditions, and execution options during workflow processing.

use crate::app::BarrierOutcome;
use crate::node::NodePartial;
use crate::runtimes::session::{SessionState, StateVersions};
use crate::types::NodeKind;

/// Result of executing one superstep in a session.
///
/// The embedded [`BarrierOutcome`] carries the canonical ordering of
/// updates/errors so callers can persist and resume without drift.
///
/// # Examples
///
/// ```rust,no_run
/// use weavegraph::runtimes::StepReport;
///
/// fn analyze_step(report: &StepReport) {
///     println!("Step {} completed", report.step);
///     println!("Ran {} nodes, skipped {}",
///              report.ran_nodes.len(),
///              report.skipped_nodes.len());
///     if report.completed {
///         println!("Workflow finished!");
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct StepReport {
    /// The step number that was executed.
    pub step: u64,
    /// Nodes that ran during this step.
    pub ran_nodes: Vec<NodeKind>,
    /// Nodes that were skipped (e.g., End nodes or version-gated).
    pub skipped_nodes: Vec<NodeKind>,
    /// The outcome from applying the barrier.
    pub barrier_outcome: BarrierOutcome,
    /// The frontier for the next step.
    pub next_frontier: Vec<NodeKind>,
    /// Channel versions after this step completed.
    pub state_versions: StateVersions,
    /// Whether the workflow has completed (reached End or empty frontier).
    pub completed: bool,
}

/// Options for controlling step execution behavior.
///
/// Use these options to implement human-in-the-loop workflows, debugging,
/// or step-by-step execution patterns.
///
/// # Examples
///
/// ```rust
/// use weavegraph::runtimes::StepOptions;
/// use weavegraph::types::NodeKind;
///
/// // Pause before a specific node
/// let options = StepOptions {
///     interrupt_before: vec![NodeKind::Custom("approval".into())],
///     interrupt_after: vec![],
///     interrupt_each_step: false,
/// };
/// ```
#[derive(Debug, Clone, Default)]
pub struct StepOptions {
    /// Nodes to pause execution before (for human-in-the-loop).
    pub interrupt_before: Vec<NodeKind>,
    /// Nodes to pause execution after.
    pub interrupt_after: Vec<NodeKind>,
    /// Whether to pause after each step (debugging mode).
    pub interrupt_each_step: bool,
}

/// The reason why execution was paused.
///
/// When a workflow is paused (not completed), this enum indicates
/// why the pause occurred, enabling appropriate handling.
#[derive(Debug, Clone)]
pub enum PausedReason {
    /// Paused before executing the specified node.
    BeforeNode(NodeKind),
    /// Paused after executing the specified node.
    AfterNode(NodeKind),
    /// Paused after completing the specified step number.
    AfterStep(u64),
}

/// Extended step report when execution is paused.
///
/// Contains the full session state at the point of pause, allowing
/// inspection, modification, or later resumption.
#[derive(Debug, Clone)]
pub struct PausedReport {
    /// The complete session state at the pause point.
    pub session_state: SessionState,
    /// Why execution was paused.
    pub reason: PausedReason,
}

/// Result of attempting to run a step.
///
/// Either the step completed normally, or execution was paused
/// for one of several reasons (human-in-the-loop, debugging, etc.).
#[derive(Debug, Clone)]
pub enum StepResult {
    /// The step completed and execution can continue.
    Completed(StepReport),
    /// Execution was paused before completion.
    Paused(PausedReport),
}

/// Internal outcome from scheduler after normalization.
///
/// Contains ordered partials ready for barrier application.
pub(crate) struct SchedulerOutcome {
    pub ran_nodes: Vec<NodeKind>,
    pub skipped_nodes: Vec<NodeKind>,
    pub partials: Vec<NodePartial>,
}
