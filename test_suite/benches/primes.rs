
#[derive(Copy, Clone)]
pub struct BenchPrimesParams {
    limit: usize,
    threads: usize,
    threads_same: bool,
}

impl core::fmt::Display for BenchPrimesParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "(Nums={} Parallel={} Same={})", self.limit, self.threads, self.threads_same)
    }
}

macro_rules! implementation {
    () => (
    impl Number {
        fn is_prime(&self) -> bool {
            self.factors.is_empty()
        }
    }

    fn factors(n: usize, nums: &Vec<Number>) -> Vec<Number> {
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
                    result.push(nums[i].clone());
                    m = m / i;
                    found = true;
                    break;
                }
            }
            if !found {
                break;
            }
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
                self.rewrite(&x.factors),
                self.rewrite(&x.last_prime),
                x.v + self.inc,
            )
        }
    }

    fn populate_numbers_single(limit: usize, v: usize) {
        let mut nums: Vec<Number> = vec![];
        let mut last_prime: Option<Number> = None;
        for n in 0..limit {
            let a: Number = Number::new(n, factors(n, &nums), last_prime.take(), v);
            if a.is_prime() && n > 0 {
                last_prime = Some(a.clone());
            }
            nums.push(a);
        }

        // Increment rewrite
        let t_inc = IncrementVBy::new(1);
        let nums2 = t_inc.rewrite(&nums);
        criterion::black_box(nums2);
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

    )
}

mod arc_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

    #[hirpdag_end(
    reference_type = "hirpdag_hashconsing::RefArc<D>",
    reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
    table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecLinearWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
    tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
    build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>",
)]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod arc_hash_sorted {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

    #[hirpdag_end(
    reference_type = "hirpdag_hashconsing::RefArc<D>",
    reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
    table_type = "hirpdag_hashconsing::TableHashmapFallbackWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>, hirpdag_hashconsing::TableVecSortedWeak<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>>",
    tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
    build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>",
)]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod arc_tovweaktable {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

    #[hirpdag_end(
    reference_type = "hirpdag_hashconsing::RefArc<D>",
    reference_weak_type = "hirpdag_hashconsing::RefArcWeak<D>",
    table_type = "hirpdag_hashconsing::TableTovWeakTable<D, hirpdag_hashconsing::RefArc<D>, hirpdag_hashconsing::RefArcWeak<D>>",
    tableshared_type = "hirpdag_hashconsing::TableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>>",
    build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>",
)]
    pub struct HirpdagEndMarker;

    implementation!();
}

mod leak_hash_linear {
    use hirpdag::*;

    #[hirpdag]
    struct Number {
        n: usize,
        factors: Vec<Number>,
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

use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};

fn bench_primes(c: &mut Criterion) {
    for limit in [1000, 8000].iter() {
        for same in [false, true].iter() {
            let name = format!("Primes{}{}", *limit, if *same {"Same"} else {""});
            let mut group = c.benchmark_group(name);
            for threads in [1, 2, 4, 8].iter() {
                let params = BenchPrimesParams { limit: *limit, threads: *threads, threads_same: *same };
                group.bench_with_input(BenchmarkId::new("ArcHashLinear", params), &params,
                    |b, params| b.iter(|| arc_hash_linear::populate_numbers(black_box(params))));
                group.bench_with_input(BenchmarkId::new("ArcHashSorted", params), &params,
                    |b, params| b.iter(|| arc_hash_sorted::populate_numbers(black_box(params))));
                group.bench_with_input(BenchmarkId::new("ArcTovWeakTable", params), &params,
                    |b, params| b.iter(|| arc_tovweaktable::populate_numbers(black_box(params))));
                group.bench_with_input(BenchmarkId::new("LeakHashLinear", params), &params,
                    |b, params| b.iter(|| leak_hash_linear::populate_numbers(black_box(params))));
            }
            group.finish();
        }
    }
}

criterion_group! {
    name = benches;
    config = Criterion::default().sample_size(25).measurement_time(core::time::Duration::from_secs(20));
    targets = bench_primes
}
criterion_main!(benches);
