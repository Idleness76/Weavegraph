#[macro_use]
extern crate proptest;

use proptest::prelude::{Strategy, prop};

// Generators shared by property tests for graphs and routing

/// Generate valid custom node names.
///
/// Constraints:
/// - Starts with a letter
/// - Followed by 0..16 of [A-Za-z0-9_]
/// - Excludes reserved endpoint names ("Start", "End")
fn node_name_strategy() -> impl Strategy<Value = String> {
    // Base regex for candidate names
    let base = prop::string::string_regex("[A-Za-z][A-Za-z0-9_]{0,16}").unwrap();
    // Filter out reserved endpoint names
    base.prop_filter("exclude reserved and reserved root name", |s| {
        s != "Start" && s != "End" && s != "Root"
    })
}

// Minimal sanity property using the generator (real graph properties will follow in later steps)
proptest! {
    #[test]
    fn prop_node_name_non_empty(name in node_name_strategy()) {
        prop_assert!(!name.is_empty());
        prop_assert!(name.chars().next().unwrap().is_ascii_alphabetic());
    }
}

mod common;
use common::*;

use proptest::prelude::any;
use rustc_hash::FxHashSet;
use std::sync::Arc;
use weavegraph::graphs::{EdgePredicate, GraphBuilder};
use weavegraph::runtimes::{AppRunner, CheckpointerType, SessionInit, StepOptions, StepResult};
use weavegraph::state::StateSnapshot;
use weavegraph::types::NodeKind;

fn block_on<F: std::future::Future<Output = ()>>(fut: F) {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt.block_on(fut);
}

proptest! {
    #[test]
    fn prop_valid_only_predicate_targets(
        mut names in prop::collection::vec(node_name_strategy(), 1..8),
        include_end in any::<bool>(),
    ) {
        // Dedup names to avoid duplicate node registrations
        names.sort();
        names.dedup();

        block_on(async move {
            // Build graph: Start -> Root -> End; register all names
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End);

            for n in &names {
                gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" });
            }

            // Predicate returns only registered names (+ optional End)
            let mut targets: Vec<String> = names.clone();
            if include_end { targets.push("End".into()); }
            let predicate: EdgePredicate = Arc::new(move |_snap| targets.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), predicate);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            let initial = state_with_user("seed");
            match runner.create_session("sess_valid".into(), initial).await.unwrap() {
                SessionInit::Fresh => {}
                _ => panic!("expected fresh session"),
            }
            let report = runner.run_step("sess_valid", StepOptions::default()).await.unwrap();
            let rep = match report { StepResult::Completed(rep) => rep, _ => panic!("expected completed") };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();

            // All predicate targets must appear (translated)
            let allowed: FxHashSet<_> = names.clone().into_iter().collect();
            for n in names.clone() {
                assert!(nf.contains(&NodeKind::Custom(n)));
            }
            if include_end { assert!(nf.contains(&NodeKind::End)); }

            // Frontier must not contain unknown custom nodes
            for k in nf {
                if let NodeKind::Custom(s) = k { assert!(allowed.contains(&s)); }
            }
        });
    }
}

proptest! {
    #[test]
    fn prop_mixed_valid_invalid_targets(
        mut valid in prop::collection::vec(node_name_strategy(), 1..6),
        mut invalid in prop::collection::vec(node_name_strategy(), 1..6),
    ) {
        // Ensure disjoint sets
        valid.sort(); valid.dedup();
        invalid.sort(); invalid.dedup();
        invalid.retain(|n| !valid.contains(n));
        prop_assume!(!valid.is_empty());
        prop_assume!(!invalid.is_empty());

        block_on(async move {
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End);
            for n in &valid { gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" }); }

            // Mixed targets: valid + invalid + maybe End
            let mut targets = valid.clone();
            targets.extend(invalid.clone());
            targets.push("End".into());
            let predicate: EdgePredicate = Arc::new(move |_snap| targets.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), predicate);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            match runner.create_session("sess_mix".into(), state_with_user("x")).await.unwrap() {
                SessionInit::Fresh => {}, _ => panic!("fresh")
            }
            let rep = match runner.run_step("sess_mix", StepOptions::default()).await.unwrap() { StepResult::Completed(rep) => rep, _ => unreachable!() };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();
            // Valid appear
            for n in &valid { assert!(nf.contains(&NodeKind::Custom(n.clone()))); }
            assert!(nf.contains(&NodeKind::End));
            // Invalid never appear
            for n in &invalid { assert!(!nf.contains(&NodeKind::Custom(n.clone()))); }
        });
    }
}

proptest! {
    #[test]
    fn prop_stress_fan_out_dedup(
        mut pool in prop::collection::vec(node_name_strategy(), 2..16),
        fanout in 1usize..64,
    ) {
        pool.sort(); pool.dedup();

        block_on(async move {
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End);
            for n in &pool { gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" }); }

            // Build predicate outputs with duplicates
            let mut outs: Vec<String> = Vec::new();
            for i in 0..fanout { outs.push(pool[i % pool.len()].clone()); }
            // Include End occasionally
            if fanout % 2 == 0 { outs.push("End".into()); }
            let predicate: EdgePredicate = Arc::new(move |_snap| outs.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), predicate);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            match runner.create_session("sess_fan".into(), state_with_user("y")).await.unwrap() {
                SessionInit::Fresh => {}, _ => panic!("fresh")
            }
            let rep = match runner.run_step("sess_fan", StepOptions::default()).await.unwrap() { StepResult::Completed(rep) => rep, _ => unreachable!() };

            // Count occurrences per custom node
            let mut counts = std::collections::HashMap::<String, usize>::new();
            for k in rep.next_frontier {
                if let NodeKind::Custom(s) = k { *counts.entry(s).or_insert(0) += 1; }
            }
            // Each targeted custom node should appear at most once
            for n in pool { assert!(counts.get(&n).cloned().unwrap_or(0) <= 1); }
        });
    }
}

// ============================================================================
// Additional property tests for conditional edge routing
// ============================================================================

/// Generate a valid key for extra data that can be used in predicates.
fn extra_key_strategy() -> impl Strategy<Value = String> {
    prop::string::string_regex("[a-z][a-z0-9_]{0,8}").unwrap()
}

proptest! {
    /// Property: Conditional edges based on extra data correctly route
    /// to the appropriate target nodes based on state content.
    #[test]
    fn prop_conditional_routing_based_on_extra_data(
        key in extra_key_strategy(),
        threshold in 0i64..100,
        value in 0i64..100,
    ) {
        block_on(async move {
            // Create predicate that routes based on extra data comparison
            let key_clone = key.clone();
            let predicate: EdgePredicate = Arc::new(move |snap: StateSnapshot| {
                if let Some(val) = snap.extra.get(&key_clone)
                    && let Some(num) = val.as_i64()
                        && num >= threshold {
                            return vec!["HighPath".to_string()];
                        }
                vec!["LowPath".to_string()]
            });

            let app = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_node(NodeKind::Custom("HighPath".into()), TestNode { name: "high" })
                .add_node(NodeKind::Custom("LowPath".into()), TestNode { name: "low" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End)
                .add_edge(NodeKind::Custom("HighPath".into()), NodeKind::End)
                .add_edge(NodeKind::Custom("LowPath".into()), NodeKind::End)
                .add_conditional_edge(NodeKind::Custom("Root".into()), predicate)
                .compile()
                .unwrap();

            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            let mut state = state_with_user("test");
            state.extra.get_mut().insert(key.clone(), serde_json::json!(value));

            runner.create_session("sess_cond".into(), state).await.unwrap();
            let rep = match runner.run_step("sess_cond", StepOptions::default()).await.unwrap() {
                StepResult::Completed(rep) => rep,
                _ => panic!("expected completed"),
            };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();

            // Verify routing matches expectation (use assert! instead of prop_assert! in async)
            if value >= threshold {
                assert!(nf.contains(&NodeKind::Custom("HighPath".into())), 
                    "expected HighPath when value {} >= threshold {}", value, threshold);
                assert!(!nf.contains(&NodeKind::Custom("LowPath".into())),
                    "unexpected LowPath when value {} >= threshold {}", value, threshold);
            } else {
                assert!(nf.contains(&NodeKind::Custom("LowPath".into())),
                    "expected LowPath when value {} < threshold {}", value, threshold);
                assert!(!nf.contains(&NodeKind::Custom("HighPath".into())),
                    "unexpected HighPath when value {} < threshold {}", value, threshold);
            }
        });
    }
}

proptest! {
    /// Property: Multiple conditional edges from the same source node
    /// independently evaluate and route to their respective targets.
    #[test]
    fn prop_multiple_conditional_edges_same_source(
        mut targets_a in prop::collection::vec(node_name_strategy(), 1..4),
        mut targets_b in prop::collection::vec(node_name_strategy(), 1..4),
    ) {
        // Ensure disjoint and dedup
        targets_a.sort(); targets_a.dedup();
        targets_b.sort(); targets_b.dedup();
        targets_b.retain(|n| !targets_a.contains(n));
        prop_assume!(!targets_a.is_empty());
        prop_assume!(!targets_b.is_empty());

        block_on(async move {
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End);

            // Register all target nodes
            for n in targets_a.iter().chain(targets_b.iter()) {
                gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" });
            }

            // First conditional edge returns targets_a
            let ta = targets_a.clone();
            let pred_a: EdgePredicate = Arc::new(move |_| ta.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), pred_a);

            // Second conditional edge returns targets_b
            let tb = targets_b.clone();
            let pred_b: EdgePredicate = Arc::new(move |_| tb.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), pred_b);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            runner.create_session("sess_multi".into(), state_with_user("x")).await.unwrap();

            let rep = match runner.run_step("sess_multi", StepOptions::default()).await.unwrap() {
                StepResult::Completed(rep) => rep,
                _ => panic!("expected completed"),
            };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();

            // Both sets of targets should appear in the frontier (use assert! in async)
            for n in &targets_a {
                assert!(nf.contains(&NodeKind::Custom(n.clone())), 
                    "expected target_a node {} in frontier", n);
            }
            for n in &targets_b {
                assert!(nf.contains(&NodeKind::Custom(n.clone())),
                    "expected target_b node {} in frontier", n);
            }
        });
    }
}

proptest! {
    /// Property: Empty predicate result should not crash and should
    /// result in no additional frontier nodes from that edge.
    #[test]
    fn prop_empty_predicate_result_safe(
        mut registered in prop::collection::vec(node_name_strategy(), 1..5),
    ) {
        registered.sort(); registered.dedup();
        prop_assume!(!registered.is_empty());

        block_on(async move {
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()))
                .add_edge(NodeKind::Custom("Root".into()), NodeKind::End);

            for n in &registered {
                gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" });
            }

            // Predicate that returns empty vec
            let pred: EdgePredicate = Arc::new(|_| Vec::new());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), pred);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            runner.create_session("sess_empty".into(), state_with_user("x")).await.unwrap();

            let rep = match runner.run_step("sess_empty", StepOptions::default()).await.unwrap() {
                StepResult::Completed(rep) => rep,
                _ => panic!("expected completed"),
            };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();

            // Should only contain End from the unconditional edge, no custom nodes
            assert!(nf.contains(&NodeKind::End), "End should be in frontier");
            for n in &registered {
                assert!(!nf.contains(&NodeKind::Custom(n.clone())),
                    "registered node {} should not be in frontier with empty predicate", n);
            }
        });
    }
}

proptest! {
    /// Property: Predicate returning "End" explicitly adds End to frontier.
    #[test]
    fn prop_predicate_can_route_to_end(
        include_end in any::<bool>(),
        mut targets in prop::collection::vec(node_name_strategy(), 0..3),
    ) {
        targets.sort(); targets.dedup();

        block_on(async move {
            let mut gb = GraphBuilder::new()
                .add_node(NodeKind::Custom("Root".into()), TestNode { name: "root" })
                .add_edge(NodeKind::Start, NodeKind::Custom("Root".into()));
            // Note: No unconditional edge to End from Root

            for n in &targets {
                gb = gb.add_node(NodeKind::Custom(n.clone()), TestNode { name: "t" });
                gb = gb.add_edge(NodeKind::Custom(n.clone()), NodeKind::End);
            }

            let mut pred_targets = targets.clone();
            if include_end {
                pred_targets.push("End".to_string());
            }
            let pred: EdgePredicate = Arc::new(move |_| pred_targets.clone());
            gb = gb.add_conditional_edge(NodeKind::Custom("Root".into()), pred);

            let app = gb.compile().unwrap();
            let mut runner = AppRunner::builder().app(app).checkpointer(CheckpointerType::InMemory).build().await;
            runner.create_session("sess_end".into(), state_with_user("x")).await.unwrap();

            let rep = match runner.run_step("sess_end", StepOptions::default()).await.unwrap() {
                StepResult::Completed(rep) => rep,
                _ => panic!("expected completed"),
            };

            let nf: FxHashSet<_> = rep.next_frontier.into_iter().collect();

            // End should be in frontier iff include_end is true (use assert_eq! in async)
            assert_eq!(nf.contains(&NodeKind::End), include_end,
                "End presence in frontier should match include_end={}", include_end);

            // All valid custom targets should be in frontier
            for n in &targets {
                assert!(nf.contains(&NodeKind::Custom(n.clone())),
                    "target {} should be in frontier", n);
            }
        });
    }
}
