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

macro_rules! implementation {
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

mod arc_hash_linear {
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

    implementation!();
}

mod leak_hash_linear {
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

    implementation!();
}

use criterion::{
    criterion_group, criterion_main, AxisScale, BenchmarkId, Criterion, PlotConfiguration,
};

fn bench_expr(c: &mut Criterion) {
    let mut group = c.benchmark_group("ExprSubstitution");
    let plot_config = PlotConfiguration::default().summary_scale(AxisScale::Logarithmic);
    group.plot_config(plot_config);
    for (depth, num_vars) in [(12usize, 4u32), (12, 16)].iter() {
        let params = BenchExprParams {
            depth: *depth,
            num_vars: *num_vars,
        };
        group.bench_with_input(
            BenchmarkId::new("ArcHashLinear", params),
            &params,
            |b, params| b.iter(|| arc_hash_linear::bench_expr(std::hint::black_box(params))),
        );
        group.bench_with_input(
            BenchmarkId::new("LeakHashLinear", params),
            &params,
            |b, params| b.iter(|| leak_hash_linear::bench_expr(std::hint::black_box(params))),
        );
    }
    group.finish();
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(10).measurement_time(core::time::Duration::from_secs(15));
    targets = bench_expr
}
criterion_main!(benches);
