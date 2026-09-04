// Benchmark: Serialization Round Trip
//
// DAG-aware serialization (`hirpdag_serialize`/`hirpdag_deserialize`
// and their JSON variants) writes each unique node exactly once into
// a topologically ordered table and re-interns on the way back in.
// No other benchmark exercises it. This one measures a full
// build -> serialize -> deserialize round trip over a highly shared
// DAG, for both the postcard binary and JSON formats.
//
// The DAG is a Fibonacci-shaped graph: `n` `Branch` nodes stacked on
// two `Leaf` nodes, each branch referencing the previous two. It has
// only `n + 2` unique nodes but exponentially many root-to-leaf
// paths, so a tree expansion would be catastrophic -- exactly the
// case DAG-aware serialization is built to handle.
//
// This is interesting because it stresses parts of the pipeline the
// construction/rewrite benchmarks don't touch:
//   1. *Serialize*: a full DAG walk that deduplicates into the node
//      table (so output stays O(unique nodes), not O(paths)).
//   2. *Deserialize*: rebuilding every node and re-interning it
//      through the hashcons table, so deserialize cost -- and how it
//      differs across table backends -- is visible.
//   3. *Format cost*: binary (postcard) vs human-readable JSON over
//      the same graph.
// Build cost is included in the measurement (consistent with the
// other whole-program benchmarks) but is O(unique nodes), the same
// order as each serialize/deserialize pass.

#[macro_use]
mod support;

#[derive(Copy, Clone, PartialEq)]
pub enum SerdeFormat {
    Binary,
    Json,
}

#[derive(Copy, Clone)]
pub struct BenchSerdeParams {
    nodes: usize,
    format: SerdeFormat,
}

impl core::fmt::Display for BenchSerdeParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let fmt = match self.format {
            SerdeFormat::Binary => "binary",
            SerdeFormat::Json => "json",
        };
        write!(f, "(nodes={} format={})", self.nodes, fmt)
    }
}

hirpdag_bench_configs! {
    // `root` makes Tree a serialization root (generates the
    // HirpdagArchiveRoots.roots_Tree field and the serialize entry points).
    #[hirpdag(root)]
    struct Tree {
        kind: TreeKind,
    }

    #[hirpdag]
    enum TreeKind {
        Leaf(u64),
        Branch(Vec<Tree>),
    }

    // Fibonacci-shaped DAG: n Branch nodes over 2 Leaf nodes.
    fn build_dag(n: usize) -> Tree {
        let mut prev = Tree::new(TreeKind::Leaf(0));
        let mut curr = Tree::new(TreeKind::Leaf(1));
        for _ in 0..n {
            let next = Tree::new(TreeKind::Branch(vec![curr.clone(), prev.clone()]));
            prev = curr;
            curr = next;
        }
        curr
    }

    pub fn serde_roundtrip(params: &crate::BenchSerdeParams) {
        let root = build_dag(params.nodes);
        let roots = HirpdagArchiveRoots {
            roots_Tree: vec![root],
            ..Default::default()
        };
        match params.format {
            crate::SerdeFormat::Binary => {
                let bytes = hirpdag_serialize(&roots).unwrap();
                let out = hirpdag_deserialize(&bytes).unwrap();
                std::hint::black_box(out);
            }
            crate::SerdeFormat::Json => {
                let text = hirpdag_serialize_json(&roots).unwrap();
                let out = hirpdag_deserialize_json(&text).unwrap();
                std::hint::black_box(out);
            }
        }
    }
}

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

const NODES: usize = 2000;
const FORMATS: [SerdeFormat; 2] = [SerdeFormat::Binary, SerdeFormat::Json];

fn bench_serde_roundtrip_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("SerdeRoundTrip");
    for format in FORMATS.iter() {
        let params = BenchSerdeParams {
            nodes: NODES,
            format: *format,
        };
        bench_each_config!(group, params, serde_roundtrip);
    }
    group.finish();
}

// Peak heap covers the transient cost the timing hides: the serialized buffer
// (much larger for JSON than for postcard) and the node table built on the way
// in, both live at once alongside the graph itself.
fn bench_serde_roundtrip_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group("SerdeRoundTripMem");
    group.sampling_mode(SamplingMode::Flat);
    for format in FORMATS.iter() {
        let params = BenchSerdeParams {
            nodes: NODES,
            format: *format,
        };
        bench_each_config_mem!(group, params, serde_roundtrip);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = support::time_criterion();
    targets = bench_serde_roundtrip_time
}

// Memory (peak-heap) benchmark; see `support::AllocBytes` and
// `bench_each_config_mem!` for the measurement and the minimum-run, fresh-table
// setup.
criterion_group! {
    name = benches_mem;
    config = support::mem_criterion();
    targets = bench_serde_roundtrip_mem
}

criterion_main!(benches_time, benches_mem);
