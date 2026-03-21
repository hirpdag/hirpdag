// Benchmark: Fibonacci DAG
//
// Creates FibNode(0), FibNode(1), ..., FibNode(N) where each
// node holds its index, the Fibonacci value, and optional
// references to FibNode(n-1) and FibNode(n-2).
//
// This is interesting because it creates a "diamond" DAG
// topology: every node (except the first two) is referenced by
// *two* parents (FibNode(n+1) and FibNode(n+2)). The same
// sub-graph is thus shared between multiple parents, which is
// the canonical use-case for hash-consing. Without structural
// sharing a naive recursive Fibonacci tree would require O(2^N)
// allocations; with hash-consing exactly N+1 unique nodes exist.
// The benchmark measures node-creation and table-lookup cost
// under this high-sharing, two-parent topology.

#[derive(Copy, Clone)]
pub struct BenchFibParams {
    n: usize,
}

impl core::fmt::Display for BenchFibParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "(N={})", self.n)
    }
}

macro_rules! fib_implementation {
    () => {
        pub fn build_fibonacci(params: &crate::BenchFibParams) {
            let mut fibs: Vec<FibNode> = Vec::with_capacity(params.n + 1);
            for i in 0..=params.n {
                let (value, prev, prev2) = match i {
                    0 => (0u64, None, None),
                    1 => (1u64, None, None),
                    n => {
                        let v = fibs[n - 1].value.saturating_add(fibs[n - 2].value);
                        (v, Some(fibs[n - 1].clone()), Some(fibs[n - 2].clone()))
                    }
                };
                fibs.push(FibNode::new(i, value, prev, prev2));
            }
            std::hint::black_box(fibs);
        }
    };
}

mod arc_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct FibNode {
        n: usize,
        value: u64,
        prev: Option<FibNode>,
        prev2: Option<FibNode>,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefArc<D>",
        reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    fib_implementation!();
}

mod leak_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct FibNode {
        n: usize,
        value: u64,
        prev: Option<FibNode>,
        prev2: Option<FibNode>,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefLeak<D>",
        reference_weak_type = "hirpdag_hashconsing::RefLeakWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    fib_implementation!();
}

use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
};

fn bench_fibonacci(c: &mut Criterion) {
    let mut group = c.benchmark_group("Fibonacci");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for n in [500usize, 2000].iter() {
        let params = BenchFibParams { n: *n };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| b.iter(|| arc_hash_linear::build_fibonacci(std::hint::black_box(params))),
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| b.iter(|| leak_hash_linear::build_fibonacci(std::hint::black_box(params))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(core::time::Duration::from_secs(30));
    targets = bench_fibonacci
}
criterion_main!(benches);
