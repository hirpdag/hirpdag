// Benchmark: Large Node Data (content-addressed blob tree)
//
// Every other benchmark in this suite uses *small* node payloads
// (a handful of integers and references).  This one deliberately
// makes the non-reference data of a node large: leaf nodes carry a
// `Vec<u8>` blob of `blob_bytes` bytes, modelling a content-addressed
// store / Merkle-DAG (think Git blobs+trees, a chunked filesystem,
// or IR constants holding large literals).
//
// The DAG structure is a balanced `fanout`-ary tree of depth
// `depth`, so there are `fanout^depth` conceptual leaf slots.  Leaf
// payloads are drawn from a pool of only `unique_blobs` distinct
// contents (leaf id cycles through `0..unique_blobs`), so:
//   * identical large blobs are deduplicated by hash-consing, and
//   * because the leaf pattern repeats, whole `Dir` subtrees higher
//     up collapse to the same interned node too.
// This keeps a genuine DAG shape while concentrating cost in the
// large payloads.
//
// This is interesting because large node data shifts where the time
// goes relative to the small-node benchmarks:
//   1. Hashing a node now hashes a large byte payload, and a
//      hash-table *hit* (the common case under sharing) still has to
//      confirm equality by comparing the full blob.  So per-node
//      intern cost is dominated by payload size, not reference
//      counting.
//   2. Deduplication of large blobs is where hash-consing pays off
//      in memory: `unique_blobs` distinct payloads are stored once
//      no matter how many leaf slots reference them.  Varying
//      `unique_blobs` sweeps from high sharing to low sharing.
//   3. A memoized rewrite that rewrites every blob (bumping the
//      first byte) must clone and re-intern each *unique* large node
//      exactly once, exercising rewrite throughput on big nodes.

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchLargeNodesParams {
    depth: usize,
    fanout: usize,
    blob_bytes: usize,
    unique_blobs: u64,
    rewrite: bool,
}

impl BenchLargeNodesParams {
    fn leaves(&self) -> usize {
        self.fanout.pow(self.depth as u32)
    }
}

impl core::fmt::Display for BenchLargeNodesParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "(leaves={} bytes={} unique={} rewrite={})",
            self.leaves(),
            self.blob_bytes,
            self.unique_blobs,
            self.rewrite
        )
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct DocNode {
        kind: DocKind,
    }

    #[hirpdag]
    enum DocKind {
        // Leaf: a large inline data payload (the "content").
        Blob(Vec<u8>),
        // Inner node: references to children (a "directory").
        Dir(Vec<DocNode>),
    }

    // Deterministic pseudo-random payload derived from `id`, so equal
    // ids always produce byte-identical (hence dedup-able) blobs.
    fn make_blob(id: u64, blob_bytes: usize) -> Vec<u8> {
        let mut v = Vec::with_capacity(blob_bytes);
        let mut state = id.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
        for _ in 0..blob_bytes {
            // SplitMix64-style step for cheap, well-mixed bytes.
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            v.push((state >> 33) as u8);
        }
        v
    }

    fn build_tree(
        depth: usize,
        fanout: usize,
        blob_bytes: usize,
        unique_blobs: u64,
        counter: &mut u64,
    ) -> DocNode {
        if depth == 0 {
            let id = *counter % unique_blobs;
            *counter += 1;
            return DocNode::new(DocKind::Blob(make_blob(id, blob_bytes)));
        }
        let children: Vec<DocNode> = (0..fanout)
            .map(|_| build_tree(depth - 1, fanout, blob_bytes, unique_blobs, counter))
            .collect();
        DocNode::new(DocKind::Dir(children))
    }

    // Memoized rewrite that bumps the first byte of every blob,
    // forcing a fresh large node per unique blob.
    struct BumpBlobs;

    impl BumpBlobs {
        fn new() -> HirpdagRewriteMemoized<Self> {
            HirpdagRewriteMemoized::new(BumpBlobs)
        }
    }

    impl HirpdagRewriter for BumpBlobs {
        fn rewrite_DocNode(&self, x: &DocNode) -> DocNode {
            if let DocKind::Blob(data) = &x.kind {
                let mut nd = data.clone();
                if let Some(b) = nd.first_mut() {
                    *b = b.wrapping_add(1);
                }
                return DocNode::new(DocKind::Blob(nd));
            }
            x.default_rewrite(self)
        }
    }

    pub fn build_large_nodes(params: &crate::BenchLargeNodesParams) {
        let mut counter = 0u64;
        let root = build_tree(
            params.depth,
            params.fanout,
            params.blob_bytes,
            params.unique_blobs,
            &mut counter,
        );
        if params.rewrite {
            let t = BumpBlobs::new();
            let rewritten = t.rewrite(&root);
            std::hint::black_box(rewritten);
        } else {
            std::hint::black_box(root);
        }
    }
}

use criterion::{criterion_group, criterion_main, Criterion};

fn bench_large_nodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("LargeNodes");
    // depth=6, fanout=4 => 4096 leaf slots. Sweep payload size,
    // sharing ratio (unique_blobs), and whether a rewrite pass runs.
    let configs = [
        // High sharing: 64 unique 1 KiB blobs referenced by 4096 slots.
        (6usize, 4usize, 1024usize, 64u64, false),
        // Low sharing: nearly all 4096 leaf slots are distinct.
        (6, 4, 1024, 4096, false),
        // Larger payloads, high sharing.
        (6, 4, 4096, 64, false),
        // High sharing plus a full-graph rewrite of every blob.
        (6, 4, 1024, 64, true),
    ];
    for (depth, fanout, blob_bytes, unique_blobs, rewrite) in configs.iter() {
        let params = BenchLargeNodesParams {
            depth: *depth,
            fanout: *fanout,
            blob_bytes: *blob_bytes,
            unique_blobs: *unique_blobs,
            rewrite: *rewrite,
        };
        bench_each_config!(group, params, build_large_nodes);
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(core::time::Duration::from_secs(15));
    targets = bench_large_nodes
}
criterion_main!(benches);
