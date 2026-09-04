// Benchmark: Churn (create + drop, reference-counting stress)
//
// Every other benchmark in this suite builds a DAG and holds it
// alive for the whole measurement. This one deliberately *destroys*
// nodes continuously, so the cost is dominated by reference-count
// decrements, `RC == 0` frees, and (for weak-table backends) table
// eviction -- the exact machinery the different `reference/`
// implementations vary.
//
// The workload keeps a rolling window of `window` live "units"
// alive. Each step builds one new unit (a small balanced tree) and,
// once the window is full, drops the oldest unit. Every unit is
// tagged with a unique serial number, so units share *no* structure
// with each other: dropping a unit really does free its whole
// subtree instead of merely decrementing a shared count. After
// `steps` steps the remaining window is dropped too.
//
// This is interesting because it isolates the destruction half of
// the lifecycle, which the build-and-hold benchmarks never exercise:
//   1. `Arc`-style counts pay an atomic decrement (and a free) per
//      node dropped; `Leak` never frees at all; the separated-count
//      layouts (`RefSep`/`RefSepPad`/`RefSepU32`) and the
//      deferred-decrement `RefTlc` are expected to separate here in
//      a way the other benchmarks cannot show.
//   2. Sweeping `window` trades the size of the live set (and thus
//      the hashcons table) against the drop rate: a small window
//      means near-immediate frees, a large window means nodes live
//      longer before being reclaimed.

#[macro_use]
mod support;

#[derive(Copy, Clone)]
pub struct BenchChurnParams {
    fanout: usize,
    depth: usize,
    window: usize,
    steps: usize,
}

impl BenchChurnParams {
    fn unit_nodes(&self) -> usize {
        // Sum of a full `fanout`-ary tree of `depth` (nodes, not leaves).
        (0..=self.depth).map(|d| self.fanout.pow(d as u32)).sum()
    }
}

impl core::fmt::Display for BenchChurnParams {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            f,
            "(unit_nodes={} window={} steps={})",
            self.unit_nodes(),
            self.window,
            self.steps
        )
    }
}

hirpdag_bench_configs! {
    #[hirpdag]
    struct ChurnNode {
        // Local index within the unit (distinguishes siblings) ...
        id: u64,
        // ... and the unit serial (distinguishes units), so no two
        // units share any node and every drop reaches RC == 0.
        tag: u64,
        children: Vec<ChurnNode>,
    }

    fn build_unit(
        serial: u64,
        fanout: usize,
        depth: usize,
        counter: &mut u64,
    ) -> ChurnNode {
        let id = *counter;
        *counter += 1;
        if depth == 0 {
            return ChurnNode::new(id, serial, vec![]);
        }
        let children: Vec<ChurnNode> = (0..fanout)
            .map(|_| build_unit(serial, fanout, depth - 1, counter))
            .collect();
        ChurnNode::new(id, serial, children)
    }

    pub fn churn(params: &crate::BenchChurnParams) {
        let mut live: std::collections::VecDeque<ChurnNode> =
            std::collections::VecDeque::with_capacity(params.window + 1);
        for serial in 0..params.steps as u64 {
            let mut counter = 0u64;
            let unit = build_unit(serial, params.fanout, params.depth, &mut counter);
            live.push_back(unit);
            if live.len() > params.window {
                // Dropping the oldest unit frees its whole subtree.
                let old = live.pop_front();
                std::hint::black_box(old);
            }
        }
        std::hint::black_box(live);
    }
}

use criterion::{criterion_group, criterion_main, Criterion, SamplingMode};

// (fanout, depth, window, steps). fanout=3, depth=3 => 40 nodes per unit.
// Sweep the live-window size: a small window drops almost immediately, a large
// one keeps far more nodes (and table entries) alive at once.
const CONFIGS: [(usize, usize, usize, usize); 2] = [(3, 3, 8, 1000), (3, 3, 256, 1000)];

fn make_params((fanout, depth, window, steps): (usize, usize, usize, usize)) -> BenchChurnParams {
    BenchChurnParams {
        fanout,
        depth,
        window,
        steps,
    }
}

fn bench_churn_time(c: &mut Criterion) {
    let mut group = c.benchmark_group("Churn");
    for config in CONFIGS.iter() {
        let params = make_params(*config);
        bench_each_config!(group, params, churn);
    }
    group.finish();
}

// Peak heap is the direct read-out of the property this benchmark is about:
// with a bounded live window, a preset that frees on `RC == 0` should peak at
// roughly window-many units no matter how many steps run, while one that never
// frees (`leak_*`) grows with `steps`. The window sweep then shows how much of
// the peak is the live set itself.
fn bench_churn_mem(c: &mut Criterion<support::AllocBytes>) {
    let mut group = c.benchmark_group("ChurnMem");
    group.sampling_mode(SamplingMode::Flat);
    for config in CONFIGS.iter() {
        let params = make_params(*config);
        bench_each_config_mem!(group, params, churn);
    }
    group.finish();
}

criterion_group! {
    name = benches_time;
    config = support::time_criterion();
    targets = bench_churn_time
}

// Memory (peak-heap) benchmark; see `support::AllocBytes` and
// `bench_each_config_mem!` for the measurement and the minimum-run, fresh-table
// setup.
criterion_group! {
    name = benches_mem;
    config = support::mem_criterion();
    targets = bench_churn_mem
}

criterion_main!(benches_time, benches_mem);
