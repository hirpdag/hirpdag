use criterion::{black_box, criterion_group, criterion_main, Criterion};
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
    build_tableshared_type = "hirpdag_hashconsing::BuildTableSharedSharded<D, hirpdag_hashconsing::RefArc<D>, ImplTable<D>, hirpdag_hashconsing::BuildTableDefault<ImplTable<D>>, std::hash::BuildHasherDefault<std::collections::hash_map::DefaultHasher>>"
)]
pub struct HirpdagEndMarker;

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

fn populate_numbers(limit: usize, v: usize) {
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
    black_box(nums2);
}

fn bench_populate_numbers(c: &mut Criterion) {
    c.bench_function("numbers 1k", |b| {
        b.iter(|| populate_numbers(black_box(1000), black_box(0)));
    });

    c.bench_function("numbers p4 1k", |b| {
        b.iter(|| {
            let mut children = vec![];
            for i in 0..4 {
                children.push(std::thread::spawn(move || {
                    populate_numbers(black_box(1000), black_box(i));
                }));
            }
            for c in children {
                let _ = c.join();
            }
        });
    });
}

fn custom_criterion() -> Criterion {
    Criterion::default().sample_size(25)
}

criterion_group! {
    name = benches;
    config = custom_criterion();
    targets = bench_populate_numbers
}
criterion_main!(benches);
