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

#[macro_use]
mod support;

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

hirpdag_bench_configs! {
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
        fn rewrite_ExprNode<D: HirpdagRewriteDriver>(
            &self,
            x: &ExprNode,
            driver: &D,
        ) -> ExprNode {
            if let ExprKind::Var(id) = &x.kind {
                if *id < self.num_vars {
                    return ExprNode::new(ExprKind::Num(u64::from(*id) + 1));
                }
            }
            x.default_rewrite(driver)
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
}

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

// (depth, num_vars): the same tree shape at two sharing ratios -- 4 variables
// collapse far more subtrees than 16 do.
const CONFIGS: [(usize, u32); 2] = [(12, 4), (12, 16)];

fn bench_expr_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("ExprSubstitution");
    for (depth, num_vars) in CONFIGS.iter() {
        let params = BenchExprParams {
            depth: *depth,
            num_vars: *num_vars,
        };
        bench_each_config!(group, params, bench_expr);
    }
    group.finish();
}

fn bench_expr_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group("ExprSubstitutionMem");
    group.sampling_mode(SamplingMode::Flat);
    for (depth, num_vars) in CONFIGS.iter() {
        let params = BenchExprParams {
            depth: *depth,
            num_vars: *num_vars,
        };
        bench_each_config_mem!(group, params, bench_expr);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = support::time_criterion();
    targets = bench_expr_time
}

// Memory (peak-heap) benchmark; see `support::AllocBytes` and
// `bench_each_config_mem!` for the measurement and the minimum-run, fresh-table
// setup.
criterion_group! {
    name = benches_mem;
    config = support::mem_criterion();
    targets = bench_expr_mem
}

criterion_main!(benches_time, benches_mem);
