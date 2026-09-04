// This benchmark builds Number nodes, from 1 up to benchmark size N.
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

hirpdag_bench_configs! {
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
        fn rewrite_Number<D: HirpdagRewriteDriver>(&self, x: &Number, driver: &D) -> Number {
            Number::new(
                x.n,
                driver.rewrite(&x.prime_factors),
                driver.rewrite(&x.last_prime),
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

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

const LIMIT: usize = 2000;

// (threads, threads_same). The first `TIME_CONFIGS` entries are the ones the
// timed group runs by default: single-threaded and saturated, for each of the
// distinct-work and same-work splits, which is what the published charts
// compare. The intermediate thread counts fill in the scaling curve and come
// back with `HIRPDAG_BENCH_SCOPE=all`; the memory group always runs them all.
const CONFIGS: [(usize, bool); 8] = [
    (1, false),
    (8, false),
    (1, true),
    (8, true),
    (2, false),
    (4, false),
    (2, true),
    (4, true),
];
const TIME_CONFIGS: usize = 4;

fn make_params(threads: usize, threads_same: bool) -> BenchPrimesParams {
    BenchPrimesParams {
        limit: LIMIT,
        threads,
        threads_same,
    }
}

// This is the config-sweep benchmark: it times *every* hash-consing preset, so
// there is always one benchmark comparing all of them. Every other benchmark
// times the key presets only (see `support::KEY_CONFIGS`).
fn bench_primes_time(c: &mut Criterion) {
    let mut group = c.benchmark_group(format!("Primes{LIMIT}"));
    for (threads, same) in support::time_params(&CONFIGS, TIME_CONFIGS) {
        let params = make_params(*threads, *same);
        bench_all_configs!(group, params, populate_numbers);
    }
    group.finish();
}

fn bench_primes_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group(format!("Primes{LIMIT}Mem"));
    group.sampling_mode(SamplingMode::Flat);
    for (threads, same) in CONFIGS.iter() {
        let params = make_params(*threads, *same);
        bench_each_config_mem!(group, params, populate_numbers);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = support::time_criterion();
    targets = bench_primes_time
}

// Memory (peak-heap) benchmark; see `support::AllocBytes` and
// `bench_each_config_mem!` for the measurement and the minimum-run, fresh-table
// setup.
criterion_group! {
    name = benches_mem;
    config = support::mem_criterion();
    targets = bench_primes_mem
}

criterion_main!(benches_time, benches_mem);
