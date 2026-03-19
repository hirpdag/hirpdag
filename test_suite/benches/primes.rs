// This benchmark Number nodes, from 1 up to benchmark size N.
// Each node has a vector of references to its prime factors (empty if prime),
// and if it is prime it has a reference to the previous prime number.

#[derive(Copy, Clone)]
pub struct BenchPrimesParams {
    limit: usize,
    threads: usize,
    threads_same: bool,
}

impl core::fmt::Display for BenchPrimesParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "(Nums={} Parallel={} Same={})",
            self.limit, self.threads, self.threads_same
        )
    }
}

macro_rules! implementation {
    () => {
        // Returns empty vector if n is prime.
        fn prime_factorize(n: usize) -> Vec<usize> {
            if n <= 3 {
                return vec![];
            }
            let mut result = vec![];
            let mut m = n;
            while m > 3 {
                let sqrt_m = (m as f64).sqrt() as usize;
                let mut found = false;
                for i in 2..=sqrt_m {
                    if (i * (m / i)) == m {
                        result.push(i);
                        m = m / i;
                        found = true;
                        break;
                    }
                }
                if !found {
                    break;
                }
            }
            if m != n {
                // The remainder is also a prime factor.
                result.push(m);
            }
            result
        }

        struct IncrementVBy {
            inc: usize,
        }

        impl IncrementVBy {
            fn new(n: usize) -> HirpdagRewriteMemoized<Self> {
                HirpdagRewriteMemoized::new(Self { inc: n })
            }
        }

        impl HirpdagRewriter for IncrementVBy {
            fn rewrite_Number(&self, x: &Number) -> Number {
                Number::new(
                    x.n,
                    self.rewrite(&x.prime_factors),
                    self.rewrite(&x.last_prime),
                    x.v + self.inc,
                )
            }
        }

        fn populate_numbers_single(limit: usize, v: usize) {
            let mut nums: Vec<Number> = vec![];
            let mut last_prime: Option<Number> = None;
            for n in 1..=limit {
                let f: Vec<Number> = prime_factorize(n)
                    .iter()
                    .map(|&n| {
                        let nn = &nums[n - 1];
                        assert_eq!(n, nn.n);
                        nn.clone()
                    })
                    .collect();
                let prime = f.is_empty() && n >= 2; // 2 is the first prime
                let a: Number = Number::new(n, f, if prime { last_prime.take() } else { None }, v);
                if prime {
                    last_prime = Some(a.clone());
                }
                nums.push(a);
            }

            // Increment v rewrite
            let t_inc = IncrementVBy::new(1);
            let nums2 = t_inc.rewrite(&nums);
            std::hint::black_box(nums2);
        }

        pub fn populate_numbers(params: &crate::BenchPrimesParams) {
            match params.threads {
                1 => {
                    populate_numbers_single(params.limit, 0);
                }
                _ => {
                    let mut children = vec![];
                    for i in 1..=params.threads {
                        let v = if params.threads_same { 0 } else { i };
                        let l = params.limit;
                        children.push(std::thread::spawn(move || {
                            populate_numbers_single(l, v);
                        }));
                    }
                    for c in children {
                        let _ = c.join();
                    }
                }
            }
        }
    };
}

mod arc_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        prime_factors: Vec<Number>,
        last_prime: Option<Number>,
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

mod arc_hash_sorted {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        prime_factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefArc<D>",
        reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecSortedWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod arc_tovweaktable {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        prime_factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefArc<D>",
        reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
        table_type = "hirpdag_hashconsing::TableTovWeakTable<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod leak_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        prime_factors: Vec<Number>,
        last_prime: Option<Number>,
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

// ===========================================================
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

mod fib_arc_hash_linear {
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

mod fib_leak_hash_linear {
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

// ===========================================================
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

macro_rules! rewrite_chain_implementation {
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

mod rewrite_chain_arc_hash_linear {
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

    rewrite_chain_implementation!();
}

mod rewrite_chain_leak_hash_linear {
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

    rewrite_chain_implementation!();
}

// ===========================================================
// Benchmark: Expression Tree with Variable Substitution
//
// Builds a balanced binary expression tree of a given depth
// whose leaves are variable nodes (Var(id), id cycling through
// 0..num_vars).  Internal nodes alternate between Add and Mul.
// Because the leaf IDs cycle, many leaf positions map to the
// *same* hash-consed Var node, and identical subtrees higher up
// are also deduplicated automatically.  After construction a
// memoized substitution rewriter replaces every Var(id) with a
// constant Num(id+1).
//
// This is interesting for two reasons:
//  1. It demonstrates *structural sharing in expression DAGs*:
//     a conceptual tree with 2^depth leaf slots may collapse to
//     far fewer unique nodes when variables repeat, exactly
//     modelling what a compiler's IR hash-consing achieves.
//  2. It demonstrates *memoization benefit in rewrites*: the
//     substitution visits each unique ExprNode exactly once
//     regardless of how many parents reference it, so the
//     effective work is O(unique nodes) rather than O(tree size).

#[derive(Copy, Clone)]
pub struct BenchExprParams {
    depth: usize,
    num_vars: u32,
}

impl core::fmt::Display for BenchExprParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "(depth={} vars={})", self.depth, self.num_vars)
    }
}

macro_rules! expr_implementation {
    () => {
        struct SubstVars {
            // Maps Var(id) -> Num(id + 1) for id in 0..num_vars.
            num_vars: u32,
        }

        impl SubstVars {
            fn new(num_vars: u32) -> HirpdagRewriteMemoized<Self> {
                HirpdagRewriteMemoized::new(SubstVars { num_vars })
            }
        }

        impl HirpdagRewriter for SubstVars {
            fn rewrite_ExprNode(&self, x: &ExprNode) -> ExprNode {
                if let ExprKind::Var(id) = &x.kind {
                    if *id < self.num_vars {
                        return ExprNode::new(ExprKind::Num(u64::from(*id) + 1));
                    }
                }
                x.default_rewrite(self)
            }
        }

        fn build_tree(depth: usize, num_vars: u32, counter: &mut u32) -> ExprNode {
            if depth == 0 {
                let id = *counter % num_vars;
                *counter += 1;
                return ExprNode::new(ExprKind::Var(id));
            }
            let left = build_tree(depth - 1, num_vars, counter);
            let right = build_tree(depth - 1, num_vars, counter);
            if depth % 2 == 0 {
                ExprNode::new(ExprKind::Add(vec![left, right]))
            } else {
                ExprNode::new(ExprKind::Mul(vec![left, right]))
            }
        }

        pub fn bench_expr(params: &crate::BenchExprParams) {
            let mut counter = 0u32;
            let root = build_tree(params.depth, params.num_vars, &mut counter);
            let sub = SubstVars::new(params.num_vars);
            let result = sub.rewrite(&root);
            std::hint::black_box(result);
        }
    };
}

mod expr_arc_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct ExprNode {
        kind: ExprKind,
    }

    #[hirpdag]
    enum ExprKind {
        Num(u64),
        Var(u32),
        Add(Vec<ExprNode>),
        Mul(Vec<ExprNode>),
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefArc<D>",
        reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    expr_implementation!();
}

mod expr_leak_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct ExprNode {
        kind: ExprKind,
    }

    #[hirpdag]
    enum ExprKind {
        Num(u64),
        Var(u32),
        Add(Vec<ExprNode>),
        Mul(Vec<ExprNode>),
    }

    #[hirpdag_end(
        reference_type = "hirpdag_hashconsing::RefLeak<D>",
        reference_weak_type = "hirpdag_hashconsing::RefLeakWeak<D>",
        table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefLeak<D>, hirpdag_hashconsing::RefLeakWeak<D>>>",
        tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>>",
        build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefLeak<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
    )]
    pub struct HirpdagEndMarker;

    expr_implementation!();
}

use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
};

fn bench_primes(c: &mut Criterion) {
    for limit in [2000].iter() {
        for same in [false, true].iter() {
            let name = format!("Primes{}{}", *limit, if *same { "Same" } else { "" });
            let mut group = c.benchmark_group(name);
            let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
            group.plot_config(plot_config);
            for threads in [1, 2, 4, 8].iter() {
                let params = BenchPrimesParams {
                    limit: *limit,
                    threads: *threads,
                    threads_same: *same,
                };
                group.bench_with_input(
                    BenchmarkId::new("ArcHashLinear", params),
                    &params,
                    |b, params| {
                        b.iter(|| arc_hash_linear::populate_numbers(std::hint::black_box(params)))
                    },
                );
                group.bench_with_input(
                    BenchmarkId::new("ArcHashSorted", params),
                    &params,
                    |b, params| {
                        b.iter(|| arc_hash_sorted::populate_numbers(std::hint::black_box(params)))
                    },
                );
                group.bench_with_input(
                    BenchmarkId::new("ArcTovWeakTable", params),
                    &params,
                    |b, params| {
                        b.iter(|| arc_tovweaktable::populate_numbers(std::hint::black_box(params)))
                    },
                );
                group.bench_with_input(
                    BenchmarkId::new("LeakHashLinear", params),
                    &params,
                    |b, params| {
                        b.iter(|| leak_hash_linear::populate_numbers(std::hint::black_box(params)))
                    },
                );
            }
            group.finish();
        }
    }
}

fn bench_fibonacci(c: &mut Criterion) {
    let mut group = c.benchmark_group("Fibonacci");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for n in [500usize, 2000].iter() {
        let params = BenchFibParams { n: *n };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| fib_arc_hash_linear::build_fibonacci(std::hint::black_box(params)))
            },
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| fib_leak_hash_linear::build_fibonacci(std::hint::black_box(params)))
            },
        );
    }
    group.finish();
}

fn bench_rewrite_chain(c: &mut Criterion) {
    let mut group = c.benchmark_group("RewriteChain");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for (length, rewrites) in [(500usize, 5usize), (500, 20), (2000, 5)].iter() {
        let params = BenchRewriteChainParams {
            length: *length,
            rewrites: *rewrites,
        };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| {
                    rewrite_chain_arc_hash_linear::bench_rewrite_chain(std::hint::black_box(params))
                })
            },
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| {
                b.iter(|| {
                    rewrite_chain_leak_hash_linear::bench_rewrite_chain(std::hint::black_box(
                        params,
                    ))
                })
            },
        );
    }
    group.finish();
}

fn bench_expr(c: &mut Criterion) {
    let mut group = c.benchmark_group("ExprSubstitution");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for (depth, num_vars) in [(10usize, 4u32), (10, 16), (12, 8)].iter() {
        let params = BenchExprParams {
            depth: *depth,
            num_vars: *num_vars,
        };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| b.iter(|| expr_arc_hash_linear::bench_expr(std::hint::black_box(params))),
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| b.iter(|| expr_leak_hash_linear::bench_expr(std::hint::black_box(params))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(core::time::Duration::from_secs(30));
    targets = bench_primes, bench_fibonacci, bench_rewrite_chain, bench_expr
}
criterion_main!(benches);
