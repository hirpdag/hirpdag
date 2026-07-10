// Shared test support.
//
// `hirpdag_test_configs!` expands the given items — a `#[hirpdag]` type plus one
// or more `#[test]` functions that exercise it — once per hash-consing
// configuration preset, each in a `#[hirpdag_module]` module named after the
// preset. Cargo then discovers the tests as `<preset>::<fn>`. The presets backed
// by third-party collection crates are gated behind the `third-party-tables`
// feature.
//
// This mirrors `benches/support/mod.rs`'s `hirpdag_bench_configs!`; its preset
// list must stay in sync with that one.

macro_rules! hirpdag_test_configs {
    (@one $module:ident, $preset:literal, $($items:item)*) => {
        #[hirpdag::hirpdag_module(preset = $preset)]
        mod $module {
            $($items)*
        }
    };
    ($($items:item)*) => {
        hirpdag_test_configs!(@one arc_hash_linear, "arc_hash_linear", $($items)*);
        hirpdag_test_configs!(@one arc_hash_sorted, "arc_hash_sorted", $($items)*);
        hirpdag_test_configs!(@one leak_hash_linear, "leak_hash_linear", $($items)*);
        hirpdag_test_configs!(@one sep_hash_linear, "sep_hash_linear", $($items)*);
        hirpdag_test_configs!(@one seppad_hash_linear, "seppad_hash_linear", $($items)*);
        hirpdag_test_configs!(@one sepu32_hash_linear, "sepu32_hash_linear", $($items)*);
        hirpdag_test_configs!(@one tlc_hash_linear, "tlc_hash_linear", $($items)*);
        // Presets backed by third-party collection crates (feature-gated).
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_tovweaktable, "arc_tovweaktable", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_dashmap, "arc_dashmap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_flurry, "arc_flurry", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_skipmap, "arc_skipmap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_arcswap, "arc_arcswap", $($items)*);
        #[cfg(feature = "third-party-tables")]
        hirpdag_test_configs!(@one arc_evmap, "arc_evmap", $($items)*);
    };
}
