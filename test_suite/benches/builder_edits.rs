// Benchmark: Builder Edits (persistent modification via to_builder)
//
// The builder API (`Foo::builder()` / `node.to_builder()`) lets you
// derive a modified copy of an existing node, hash-consing once at
// the end. No other benchmark exercises it. This one measures
// *persistent editing*: repeatedly producing a new version of a tree
// that differs from the previous version by a single leaf value,
// while keeping every past version alive.
//
// A balanced `fanout`-ary tree of `depth` is built once. Then each
// edit walks a root-to-leaf path and rebuilds exactly that path with
// `to_builder()` at every level, replacing one child vector per node
// -- the classic persistent (path-copying) update. Each edit chains
// onto the previous version and all versions are retained.
//
// This is interesting because it exercises two properties at once:
//   1. *Builder overhead*: every edit performs `depth` builder
//      round-trips (clone the child vector, swap one entry, `build()`
//      to re-intern), so the benchmark isolates the per-node builder
//      + intern cost along a path.
//   2. *Structural sharing under persistence*: an edit allocates only
//      O(depth) new nodes; the entire rest of the tree is shared with
//      the prior version. Holding all versions alive keeps that shared
//      history resident, which is exactly what a persistent data
//      structure is meant to make cheap.

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchBuilderEditsParams {
    fanout: usize,
    depth: usize,
    edits: usize,
}

impl BenchBuilderEditsParams {
    fn leaves(&self) -> usize {
        self.fanout.pow(self.depth as u32)
    }
}

impl core::fmt::Display for BenchBuilderEditsParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "(leaves={} depth={} edits={})",
            self.leaves(),
            self.depth,
            self.edits
        )
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct TreeNode {
        value: u64,
        children: Vec<TreeNode>,
    }

    fn build_tree(fanout: usize, depth: usize, counter: &mut u64) -> TreeNode {
        let value = *counter;
        *counter += 1;
        if depth == 0 {
            return TreeNode::new(value, vec![]);
        }
        let children: Vec<TreeNode> = (0..fanout)
            .map(|_| build_tree(fanout, depth - 1, counter))
            .collect();
        TreeNode::new(value, children)
    }

    // Path of `depth` child indices, derived from `k` so successive
    // edits touch different leaves (mixed-radix over `fanout`).
    fn nth_path(mut k: u64, fanout: usize, depth: usize) -> Vec<usize> {
        (0..depth)
            .map(|_| {
                let i = (k % fanout as u64) as usize;
                k /= fanout as u64;
                i
            })
            .collect()
    }

    // Persistent update: rebuild only the nodes on `path`, using
    // to_builder at each level; everything else is shared.
    fn update_path(node: &TreeNode, path: &[usize], new_value: u64) -> TreeNode {
        match path.split_first() {
            None => node.to_builder().value(new_value).build(),
            Some((&i, rest)) => {
                let mut children = node.children.clone();
                children[i] = update_path(&node.children[i], rest, new_value);
                node.to_builder().children(children).build()
            }
        }
    }

    pub fn builder_edits(params: &crate::BenchBuilderEditsParams) {
        let mut counter = 0u64;
        let root = build_tree(params.fanout, params.depth, &mut counter);
        // Retain every version so the persistent history stays resident.
        let mut versions: Vec<TreeNode> = Vec::with_capacity(params.edits + 1);
        versions.push(root);
        for k in 0..params.edits as u64 {
            let path = nth_path(k, params.fanout, params.depth);
            let updated = {
                let current = versions.last().unwrap();
                update_path(current, &path, k + 1)
            };
            versions.push(updated);
        }
        std::hint::black_box(versions);
    }
}

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_builder_edits(c: &mut Criterion) {
    let mut group = c.benchmark_group("BuilderEdits");
    // fanout=2, depth=10 => 1024-leaf tree; each edit rebuilds a
    // depth-long path. Sweep the number of edits and the tree shape.
    let configs = [(2usize, 10usize, 500usize), (4, 6, 500)];
    for (fanout, depth, edits) in configs.iter() {
        let params = BenchBuilderEditsParams {
            fanout: *fanout,
            depth: *depth,
            edits: *edits,
        };
        bench_each_config!(group, params, builder_edits);
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(core::time::Duration::from_secs(15));
    targets = bench_builder_edits
}
criterion_main!(benches);
