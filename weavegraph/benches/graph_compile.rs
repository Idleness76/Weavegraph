//! Benchmarks for graph compilation and validation.
//!
//! These benchmarks measure the performance of:
//! - Graph building and compilation
//! - Validation algorithms (cycle detection, reachability)
//! - Topological sort
//! - petgraph conversion (when feature enabled)

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use weavegraph::graphs::GraphBuilder;
use weavegraph::node::{Node, NodeContext, NodeError, NodePartial};
use weavegraph::state::StateSnapshot;
use weavegraph::types::NodeKind;

/// A minimal no-op node for benchmarking graph structure operations.
struct BenchNode;

#[async_trait::async_trait]
impl Node for BenchNode {
    async fn run(&self, _: StateSnapshot, _: NodeContext) -> Result<NodePartial, NodeError> {
        Ok(NodePartial::default())
    }
}

/// Build a linear graph: Start -> N1 -> N2 -> ... -> Nn -> End
fn build_linear_graph(node_count: usize) -> GraphBuilder {
    let mut builder = GraphBuilder::new();

    // Add all nodes
    for i in 0..node_count {
        builder = builder.add_node(NodeKind::Custom(format!("node_{i}")), BenchNode);
    }

    // Add edges: Start -> node_0
    if node_count > 0 {
        builder = builder.add_edge(NodeKind::Start, NodeKind::Custom("node_0".into()));
    }

    // Add edges: node_i -> node_{i+1}
    for i in 0..node_count.saturating_sub(1) {
        builder = builder.add_edge(
            NodeKind::Custom(format!("node_{i}")),
            NodeKind::Custom(format!("node_{}", i + 1)),
        );
    }

    // Add edge to End
    if node_count > 0 {
        builder = builder.add_edge(
            NodeKind::Custom(format!("node_{}", node_count - 1)),
            NodeKind::End,
        );
    } else {
        builder = builder.add_edge(NodeKind::Start, NodeKind::End);
    }

    builder
}

/// Build a fan-out/fan-in graph:
/// Start -> [N parallel nodes] -> End
fn build_fanout_graph(width: usize) -> GraphBuilder {
    let mut builder = GraphBuilder::new();

    for i in 0..width {
        builder = builder
            .add_node(NodeKind::Custom(format!("worker_{i}")), BenchNode)
            .add_edge(NodeKind::Start, NodeKind::Custom(format!("worker_{i}")))
            .add_edge(NodeKind::Custom(format!("worker_{i}")), NodeKind::End);
    }

    builder
}

/// Build a diamond/DAG graph with multiple paths
fn build_diamond_graph(depth: usize, width: usize) -> GraphBuilder {
    let mut builder = GraphBuilder::new();

    // Create layers of nodes
    for layer in 0..depth {
        for node in 0..width {
            let name = format!("L{layer}_N{node}");
            builder = builder.add_node(NodeKind::Custom(name), BenchNode);
        }
    }

    // Connect Start to first layer
    for node in 0..width {
        builder = builder.add_edge(NodeKind::Start, NodeKind::Custom(format!("L0_N{node}")));
    }

    // Connect layers (each node connects to all nodes in next layer)
    for layer in 0..depth.saturating_sub(1) {
        for from_node in 0..width {
            let from = NodeKind::Custom(format!("L{layer}_N{from_node}"));
            // Connect to subset of next layer to avoid explosion
            let to_node = from_node % width;
            let to = NodeKind::Custom(format!("L{}_N{to_node}", layer + 1));
            builder = builder.add_edge(from, to);
        }
    }

    // Connect last layer to End
    let last_layer = depth.saturating_sub(1);
    for node in 0..width {
        builder = builder.add_edge(
            NodeKind::Custom(format!("L{last_layer}_N{node}")),
            NodeKind::End,
        );
    }

    builder
}

fn bench_graph_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_compile");

    // Linear graphs
    for size in [10, 50, 100, 200] {
        group.bench_with_input(BenchmarkId::new("linear", size), &size, |b, &size| {
            b.iter(|| {
                let builder = build_linear_graph(size);
                builder.compile().expect("compilation should succeed")
            });
        });
    }

    // Fan-out graphs
    for width in [10, 50, 100] {
        group.bench_with_input(BenchmarkId::new("fanout", width), &width, |b, &width| {
            b.iter(|| {
                let builder = build_fanout_graph(width);
                builder.compile().expect("compilation should succeed")
            });
        });
    }

    // Diamond/DAG graphs
    for (depth, width) in [(5, 10), (10, 10), (5, 20)] {
        group.bench_with_input(
            BenchmarkId::new("diamond", format!("{depth}x{width}")),
            &(depth, width),
            |b, &(depth, width)| {
                b.iter(|| {
                    let builder = build_diamond_graph(depth, width);
                    builder.compile().expect("compilation should succeed")
                });
            },
        );
    }

    group.finish();
}

fn bench_topological_sort(c: &mut Criterion) {
    let mut group = c.benchmark_group("topological_sort");

    for size in [10, 50, 100, 200] {
        let builder = build_linear_graph(size);

        group.bench_with_input(BenchmarkId::new("linear", size), &builder, |b, builder| {
            b.iter(|| builder.topological_sort());
        });
    }

    for (depth, width) in [(5, 10), (10, 10), (5, 20)] {
        let builder = build_diamond_graph(depth, width);

        group.bench_with_input(
            BenchmarkId::new("diamond", format!("{depth}x{width}")),
            &builder,
            |b, builder| {
                b.iter(|| builder.topological_sort());
            },
        );
    }

    group.finish();
}

fn bench_iterators(c: &mut Criterion) {
    let mut group = c.benchmark_group("graph_iterators");

    for size in [10, 50, 100] {
        let builder = build_linear_graph(size);

        group.bench_with_input(
            BenchmarkId::new("nodes_iter", size),
            &builder,
            |b, builder| {
                b.iter(|| builder.nodes().count());
            },
        );

        group.bench_with_input(
            BenchmarkId::new("edges_iter", size),
            &builder,
            |b, builder| {
                b.iter(|| builder.edges().count());
            },
        );
    }

    group.finish();
}

#[cfg(feature = "petgraph-compat")]
fn bench_petgraph_conversion(c: &mut Criterion) {
    let mut group = c.benchmark_group("petgraph_compat");

    for size in [10, 50, 100] {
        let builder = build_linear_graph(size);

        group.bench_with_input(
            BenchmarkId::new("to_petgraph", size),
            &builder,
            |b, builder| {
                b.iter(|| builder.to_petgraph());
            },
        );

        group.bench_with_input(BenchmarkId::new("to_dot", size), &builder, |b, builder| {
            b.iter(|| builder.to_dot());
        });

        group.bench_with_input(
            BenchmarkId::new("is_cyclic_petgraph", size),
            &builder,
            |b, builder| {
                b.iter(|| builder.is_cyclic_petgraph());
            },
        );
    }

    group.finish();
}

#[cfg(feature = "petgraph-compat")]
criterion_group!(
    benches,
    bench_graph_compile,
    bench_topological_sort,
    bench_iterators,
    bench_petgraph_conversion,
);

#[cfg(not(feature = "petgraph-compat"))]
criterion_group!(
    benches,
    bench_graph_compile,
    bench_topological_sort,
    bench_iterators,
);

criterion_main!(benches);
