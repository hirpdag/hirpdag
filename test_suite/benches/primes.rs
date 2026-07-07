// This benchmark Number nodes, from 1 up to benchmark size N.
// Each node has a vector of references to its prime factors (empty if prime),
// and if it is prime it has a reference to the previous prime number.

#[macro_use]
mod support;

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

hirpdag::hirpdag_configurations! {
    configurations = [arc_hash_linear, arc_hash_sorted, arc_tovweaktable, leak_hash_linear];

    #[hirpdag]
    struct Number {
        n: usize,
        prime_factors: Vec<Number>,
        last_prime: Option<Number>,
        v: usize,
    }

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
}

use criterion::Criterion;

fn bench_primes(c: &mut Criterion) {
    for limit in [2000].iter() {
        for same in [false, true].iter() {
            let name = format!("Primes{}{}", *limit, if *same { "Same" } else { "" });
            let mut group = support::log_scale_group(c, &name);
            for threads in [1, 2, 4, 8].iter() {
                let params = BenchPrimesParams {
                    limit: *limit,
                    threads: *threads,
                    threads_same: *same,
                };
                bench_each_config!(group, params, populate_numbers);
            }
            group.finish();
        }
    }
}

hirpdag_bench_main!(bench_primes);
