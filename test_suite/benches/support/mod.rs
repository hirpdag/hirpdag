// Shared benchmark support.
//
// `hirpdag_bench_configs!` expands the given items (the `#[hirpdag]` type
// definitions and the benchmark implementation) once per hash-consing
// configuration preset, each in a `#[hirpdag_module]` module named after
// the preset. `bench_each_config!` registers a criterion benchmark for a
// function from each of those modules; its module/label list must stay in
// sync with the preset list in `hirpdag_bench_configs!`.

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
        hirpdag_bench_configs!(@one leak_hash_linear, "leak_hash_linear", $($items)*);
        hirpdag_bench_configs!(@one sep_hash_linear, "sep_hash_linear", $($items)*);
        hirpdag_bench_configs!(@one seppad_hash_linear, "seppad_hash_linear", $($items)*);
        hirpdag_bench_configs!(@one sepu32_hash_linear, "sepu32_hash_linear", $($items)*);
        hirpdag_bench_configs!(@one tlc_hash_linear, "tlc_hash_linear", $($items)*);
        // Tables backed by third-party collection crates (feature-gated).
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_tovweaktable, "arc_tovweaktable", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_dashmap, "arc_dashmap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_flurry, "arc_flurry", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_skipmap, "arc_skipmap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_arcswap, "arc_arcswap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_bench_configs!(@one arc_evmap, "arc_evmap", $($items)*);
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
        bench_each_config!(@one $group, $params, $function, leak_hash_linear, "LeakHashLinear");
        bench_each_config!(@one $group, $params, $function, sep_hash_linear, "SepHashLinear");
        bench_each_config!(@one $group, $params, $function, seppad_hash_linear, "SepPadHashLinear");
        bench_each_config!(@one $group, $params, $function, sepu32_hash_linear, "SepU32HashLinear");
        bench_each_config!(@one $group, $params, $function, tlc_hash_linear, "TlcHashLinear");
        // Tables backed by third-party collection crates (feature-gated).
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_tovweaktable, "ArcTovWeakTable");
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_dashmap, "ArcDashMap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_flurry, "ArcFlurry");
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_skipmap, "ArcSkipMap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_arcswap, "ArcArcSwap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config!(@one $group, $params, $function, arc_evmap, "ArcEvmap");
    };
}
