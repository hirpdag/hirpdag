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

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

fn bench_primes_time(c: &mut Criterion) {
    for limit in [2000].iter() {
        let name = format!("Primes{}", *limit);
        let mut group = c.benchmark_group(name);
        for same in [false, true].iter() {
            for threads in [1, 2, 4, 8].iter() {
                let params = BenchPrimesParams {
                    limit: *limit,
                    threads: *threads,
                    threads_same: *same,
                };
                bench_each_config!(group, params, populate_numbers);
            }
        }
        group.finish();
    }
}

fn bench_primes_mem(c: &mut Criterion<support::AllocBytes>) {
    for limit in [2000].iter() {
        let name = format!("Primes{}Mem", *limit);
        let mut group = c.benchmark_group(name);
        group.sampling_mode(SamplingMode::Flat);
        for same in [false, true].iter() {
            for threads in [1, 2, 4, 8].iter() {
                let params = BenchPrimesParams {
                    limit: *limit,
                    threads: *threads,
                    threads_same: *same,
                };
                bench_each_config_mem!(group, params, populate_numbers);
            }
        }
        group.finish();
    }
}

criterion_group! {
    name = benches_time;
    config = Criterion::default()
        .sample_size(10)
        .measurement_time(core::time::Duration::from_secs(15));
    targets = bench_primes_time
}

// Memory (peak-heap) benchmark. `bench_each_config_mem!` resets the interning
// table before each measured build, so every run starts from empty. Peak-heap
// sizes are deterministic, so this is configured for the minimum number of runs
// criterion allows (flat sampling with a tiny warm-up and measurement window,
// making each of the ten samples a single invocation) and `without_plots()`
// because criterion cannot render a distribution from zero-variance samples.
criterion_group! {
    name = benches_mem;
    config = Criterion::default()
        .with_measurement(support::AllocBytes)
        .without_plots()
        .sample_size(10)
        .warm_up_time(core::time::Duration::from_nanos(1))
        .measurement_time(core::time::Duration::from_nanos(1));
    targets = bench_primes_mem
}

criterion_main!(benches_time, benches_mem);
