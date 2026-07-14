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
        fn rewrite_ChainLink(&self, x: &ChainLink) -> ChainLink {
            ChainLink::new(x.n, self.rewrite(&x.next), x.v + 1)
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

fn bench_rewrite_chain_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("RewriteChain");
    for (length, rewrites) in [(500usize, 20usize), (2000, 5)].iter() {
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
    for (length, rewrites) in [(500usize, 20usize), (2000, 5)].iter() {
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
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(core::time::Duration::from_secs(15));
    targets = bench_rewrite_chain_time
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
        .warm_up_time(core::time::Duration::from_millis(1))
        .measurement_time(core::time::Duration::from_millis(1));
    targets = bench_rewrite_chain_mem
}

criterion_main!(benches_time, benches_mem);
