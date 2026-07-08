// Shared benchmark support.
//
// `hirpdag_bench_configs!` expands the given items (the `#[hirpdag]` type
// definitions and the benchmark implementation) once per hash-consing
// configuration preset, each in a `#[hirpdag_module]` module named after
// the preset. `bench_each_config!` registers a criterion benchmark for a
// function from each of those modules; its module/label list must stay in
// sync with the preset list in `hirpdag_bench_configs!`.
// `hirpdag_bench_main!` is the shared criterion_group/criterion_main
// boilerplate.

macro_rules! hirpdag_bench_configs {
    (@one $module:ident, $preset:literal, $($items:item)*) => {
        #[hirpdag::hirpdag_module(preset = $preset)]
        mod $module {
            $($items)*
        }
    };
    ($($items:item)*) => {
        hirpdag_bench_configs!(@one arc_hash_linear, "arc_hash_linear", $($items)*);
        hirpdag_bench_configs!(@one arc_hash_sorted, "arc_hash_sorted", $($items)*);
        hirpdag_bench_configs!(@one arc_tovweaktable, "arc_tovweaktable", $($items)*);
        hirpdag_bench_configs!(@one leak_hash_linear, "leak_hash_linear", $($items)*);
    };
}

macro_rules! bench_each_config {
    (@one $group:expr, $params:expr, $function:ident, $module:ident, $label:literal) => {
        $group.bench_with_input(
            criterion::BenchmarkId::new($label, $params),
            &$params,
            |b, params| b.iter(|| crate::$module::$function(std::hint::black_box(params))),
        );
    };
    ($group:expr, $params:expr, $function:ident) => {
        bench_each_config!(@one $group, $params, $function, arc_hash_linear, "ArcHashLinear");
        bench_each_config!(@one $group, $params, $function, arc_hash_sorted, "ArcHashSorted");
        bench_each_config!(@one $group, $params, $function, arc_tovweaktable, "ArcTovWeakTable");
        bench_each_config!(@one $group, $params, $function, leak_hash_linear, "LeakHashLinear");
    };
}

// Criterion group/main boilerplate with the sample size and measurement time
// shared by all hirpdag benchmarks.
macro_rules! hirpdag_bench_main {
    ($($target:ident),+ $(,)?) => {
        criterion::criterion_group! {
            name = benches;
            config = criterion::Criterion::default()
                .sample_size(10)
                .measurement_time(core::time::Duration::from_secs(15));
            targets = $($target),+
        }
        criterion::criterion_main!(benches);
    };
}

use criterion::{measurement::WallTime, AxisScale, BenchmarkGroup, PlotConfiguration};

pub fn configure_log_scale(group: &mut BenchmarkGroup<'_, WallTime>) {
    group.plot_config(PlotConfiguration::default().summary_scale(AxisScale::Logarithmic));
}
