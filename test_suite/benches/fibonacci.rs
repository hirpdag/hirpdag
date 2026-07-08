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

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchFibParams {
    n: usize,
}

impl core::fmt::Display for BenchFibParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "(N={})", self.n)
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct FibNode {
        n: usize,
        value: u64,
        prev: Option<FibNode>,
        prev2: Option<FibNode>,
    }

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
}

use criterion::Criterion;

fn bench_fibonacci(c: &mut Criterion) {
    let mut group = c.benchmark_group("Fibonacci");
    support::configure_log_scale(&mut group);
    for n in [200usize, 2000].iter() {
        let params = BenchFibParams { n: *n };
        bench_each_config!(group, params, build_fibonacci);
    }
    group.finish();
}

hirpdag_bench_main!(bench_fibonacci);
