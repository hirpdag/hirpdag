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

macro_rules! implementation {
    () => {
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
    };
}

mod arc_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct ChainLink {
        n: usize,
        next: Option<ChainLink>,
        v: usize,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefArc<D>",
        reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod leak_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct ChainLink {
        n: usize,
        next: Option<ChainLink>,
        v: usize,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefLeak<D>",
        reference_weak_type = "hirpdag_hashconsing::RefLeakWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    implementation!();
}

use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
};

fn bench_rewrite_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("RewriteChain");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for (length, rewrites) in [(500usize, 20usize), (2000, 5)].iter() {
        let params = BenchRewriteChainParams {
            length: *length,
            rewrites: *rewrites,
        };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| arc_hash_linear::bench_rewrite_chain(std::hint::black_box(params)))
            },
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| leak_hash_linear::bench_rewrite_chain(std::hint::black_box(params)))
            },
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(core::time::Duration::from_secs(15));
    targets = bench_rewrite_chain
}
criterion_main!(benches);
