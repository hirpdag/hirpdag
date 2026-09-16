// Benchmark: Rewrite Chain
//
// Builds a singly-linked chain of ChainLink nodes, each holding
// an index `n`, a version counter `v`, and an optional reference
// to the next link.  Then applies K sequential rewrites; every
// rewrite increments `v` by 1 for every node in the chain,
// producing a fresh set of interned nodes with `v = k`.
//
// This is interesting because it stresses the *rewrite
// infrastructure* in isolation from DAG construction.  With K
// rewrites on a chain of N nodes there are K*N new intern
// lookups; because each (n, v) pair is unique, the hash-consing
// table cannot short-circuit any of them on the first pass.
// Comparing Arc vs Leak reference types shows the overhead of
// reference-counting vs leak-allocating under repeated full-graph
// rewrites.  Comparing different K values reveals how the cost
// scales linearly with the number of rewrite steps.

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchRewriteChainParams {
    length: usize,
    rewrites: usize,
}

impl core::fmt::Display for BenchRewriteChainParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "(N={} K={})", self.length, self.rewrites)
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct ChainLink {
        n: usize,
        next: Option<ChainLink>,
        v: usize,
    }

    struct BumpV;

    impl BumpV {
        fn new() -> HirpdagRewriteMemoized<Self> {
            HirpdagRewriteMemoized::new(BumpV)
        }
    }

    impl HirpdagRewriter for BumpV {
        fn rewrite_ChainLink<D: HirpdagRewriteDriver>(
            &self,
            x: &ChainLink,
            driver: &D,
        ) -> ChainLink {
            ChainLink::new(x.n, driver.rewrite(&x.next), x.v + 1)
        }
    }

    pub fn bench_rewrite_chain(params: &crate::BenchRewriteChainParams) {
        // Build an N-node chain with v=0.
        let mut head: Option<ChainLink> = None;
        for i in 0..params.length {
            head = Some(ChainLink::new(i, head, 0));
        }
        // Apply K rewrites sequentially, each bumping v by 1.
        let mut current = head;
        for _ in 0..params.rewrites {
            let t = BumpV::new();
            current = t.rewrite(&current);
        }
        std::hint::black_box(current);
    }
}

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

// (length, rewrites): a short chain rewritten many times, and a long chain
// rewritten a few times. Both are timed by default; the memory group runs them
// as well.
const CONFIGS: [(usize, usize); 2] = [(500, 20), (2000, 5)];

fn bench_rewrite_chain_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("RewriteChain");
    for (length, rewrites) in CONFIGS.iter() {
        let params = BenchRewriteChainParams {
            length: *length,
            rewrites: *rewrites,
        };
        bench_each_config!(group, params, bench_rewrite_chain);
    }
    group.finish();
}

fn bench_rewrite_chain_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group("RewriteChainMem");
    group.sampling_mode(SamplingMode::Flat);
    for (length, rewrites) in CONFIGS.iter() {
        let params = BenchRewriteChainParams {
            length: *length,
            rewrites: *rewrites,
        };
        bench_each_config_mem!(group, params, bench_rewrite_chain);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = support::time_criterion();
    targets = bench_rewrite_chain_time
}

// Memory (peak-heap) benchmark; see `support::AllocBytes` and
// `bench_each_config_mem!` for the measurement and the minimum-run, fresh-table
// setup.
criterion_group! {
    name = benches_mem;
    config = support::mem_criterion();
    targets = bench_rewrite_chain_mem
}

criterion_main!(benches_time, benches_mem);
