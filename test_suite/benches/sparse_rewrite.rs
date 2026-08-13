// Benchmark: Sparse Rewrite (mostly-unchanged rewrite pass)
//
// Applies a rewrite that touches only a fraction of the nodes, exercising the
// "no changes" fast path in the macro-generated `default_rewrite`: when
// rewriting a node's fields yields values equal to the originals, the input
// reference is cloned (one ref-count bump) instead of rebuilding the node and
// going through the hash-consing table.
//
// The tree is balanced binary with `Option<TreeNode>` children (no `Vec`), so an
// unchanged subtree hits the fast path with zero allocation and no table lookup.
// Node ids are unique (pre-order), so there is no sharing: `2^(depth+1) - 1`
// distinct nodes.
//
// `change_period` sweeps how sparse the rewrite is:
//   * `1` — every node is a change seed, so every node changes.
//   * `k` — roughly one node in `k` is a seed; a subtree is rebuilt only if it
//     contains a seed, so most subtrees are cloned unchanged.
//   * `0` — no seeds: a pure no-op rewrite that clones the whole tree back.
//
// Cost drops sharply as `change_period` grows and fewer nodes change.

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchSparseRewriteParams {
    depth: usize,
    rewrites: usize,
    change_period: u64,
}

impl BenchSparseRewriteParams {
    fn nodes(&self) -> usize {
        (1usize << (self.depth + 1)) - 1
    }
}

impl core::fmt::Display for BenchSparseRewriteParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "(nodes={} K={} change_period={})",
            self.nodes(),
            self.rewrites,
            self.change_period
        )
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct TreeNode {
        // Unique per structural position (assigned in pre-order at build time);
        // selects which nodes the rewrite treats as change seeds.
        id: u64,
        // Version counter, bumped by the rewrite to produce a fresh node.
        v: u64,
        left: Option<TreeNode>,
        right: Option<TreeNode>,
    }

    // A rewrite that bumps the version of every node whose id is a multiple of
    // `change_period`, and leaves every other node untouched. A node is
    // reconstructed only if it is a seed or if one of its subtrees changed;
    // otherwise `default_rewrite` returns a clone of the input reference.
    struct BumpSome {
        change_period: u64,
    }

    impl BumpSome {
        fn new(change_period: u64) -> HirpdagRewriteMemoized<Self> {
            HirpdagRewriteMemoized::new(BumpSome { change_period })
        }
    }

    impl HirpdagRewriter for BumpSome {
        fn rewrite_TreeNode(&self, x: &TreeNode) -> TreeNode {
            // Rewrite the children first. Unchanged subtrees are cloned via the
            // default_rewrite fast path (no allocation, no table lookup).
            let rewritten = x.default_rewrite(self);
            if self.change_period != 0 && x.id % self.change_period == 0 {
                // This node is a change seed: bump its version, producing a fresh node.
                return TreeNode::new(
                    rewritten.id,
                    rewritten.v + 1,
                    rewritten.left.clone(),
                    rewritten.right.clone(),
                );
            }
            rewritten
        }
    }

    fn build_tree(depth: usize, counter: &mut u64) -> TreeNode {
        let id = *counter;
        *counter += 1;
        if depth == 0 {
            return TreeNode::new(id, 0, None, None);
        }
        let left = build_tree(depth - 1, counter);
        let right = build_tree(depth - 1, counter);
        TreeNode::new(id, 0, Some(left), Some(right))
    }

    pub fn bench_sparse_rewrite(params: &crate::BenchSparseRewriteParams) {
        let mut counter = 0u64;
        let root = build_tree(params.depth, &mut counter);
        // Apply K rewrite passes. With change_period == 0 every pass is a no-op
        // returning the same interned tree; otherwise each pass rebuilds only the
        // seed nodes and their ancestors.
        let mut current = root;
        for _ in 0..params.rewrites {
            let t = BumpSome::new(params.change_period);
            current = t.rewrite(&current);
        }
        std::hint::black_box(current);
    }
}

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

// depth=14 => 32767 nodes. Sweep from a full-graph rewrite (change_period=1)
// through a sparse one (change_period=16) to a pure no-op (change_period=0).
const CONFIGS: [(usize, usize, u64); 3] = [
    (14, 8, 1),  // full: every node changes every pass
    (14, 8, 16), // sparse: ~1 node in 16 is a change seed
    (14, 8, 0),  // none: pure no-op rewrite, whole tree cloned back
];

fn bench_sparse_rewrite_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("SparseRewrite");
    for (depth, rewrites, change_period) in CONFIGS.iter() {
        let params = BenchSparseRewriteParams {
            depth: *depth,
            rewrites: *rewrites,
            change_period: *change_period,
        };
        bench_each_config!(group, params, bench_sparse_rewrite);
    }
    group.finish();
}

fn bench_sparse_rewrite_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group("SparseRewriteMem");
    group.sampling_mode(SamplingMode::Flat);
    for (depth, rewrites, change_period) in CONFIGS.iter() {
        let params = BenchSparseRewriteParams {
            depth: *depth,
            rewrites: *rewrites,
            change_period: *change_period,
        };
        bench_each_config_mem!(group, params, bench_sparse_rewrite);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(core::time::Duration::from_secs(15));
    targets = bench_sparse_rewrite_time
}

// Memory (peak-heap) benchmark. `bench_each_config_mem!` resets the interning
// table before each measured build, so every run starts from empty. Peak-heap
// sizes are deterministic, so this is configured for the minimum number of runs
// criterion allows (flat sampling with a tiny warm-up and measurement window,
// making each of the ten samples a single invocation) and `without_plots()`
// because criterion cannot render a distribution from zero-variance samples.
criterion_group! {
    name = benches_mem;
    config = Criterion::default()
        .with_measurement(support::AllocBytes)
        .without_plots()
        .sample_size(10)
        .warm_up_time(core::time::Duration::from_nanos(1))
        .measurement_time(core::time::Duration::from_nanos(1));
    targets = bench_sparse_rewrite_mem
}

criterion_main!(benches_time, benches_mem);
