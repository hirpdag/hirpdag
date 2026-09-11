// Shared test support.
//
// `hirpdag_test_configs!` expands the given items (a `#[hirpdag]` type plus one
// or more `#[test]` functions that exercise it) once per hash-consing
// configuration preset, each in a `#[hirpdag_module]` module named after the
// preset. Cargo then discovers the tests as `<preset>::<fn>`.
//
// The preset list itself lives in one place, `hirpdag_derive::presets`, and is
// driven from here by `hirpdag::hirpdag_for_each_preset!`. Adding a preset
// there adds it to these tests, to the benchmarks, and to the macro's own
// validation, with no list to keep in step. The presets backed by third-party
// collection crates carry a `#[cfg(feature = "third-party-tables")]` that the
// driver emits and this crate's feature of that name resolves.

/// The per-preset callback `hirpdag_for_each_preset!` invokes. One arm: the
/// driver always supplies the module identifier, preset name and display
/// label, then the payload.
// The `_one` callbacks are an implementation detail of the macro above them.
// If the public macro goes unused, that is the warning worth seeing; a second
// one for its callback is noise.
#[allow(unused_macros)]
macro_rules! hirpdag_test_configs_one {
    ($module:ident, $preset:literal, $label:literal, $($items:item)*) => {
        #[hirpdag::hirpdag_module(preset = $preset)]
        mod $module {
            $($items)*
        }
    };
}

macro_rules! hirpdag_test_configs {
    ($($items:item)*) => {
        hirpdag::hirpdag_for_each_preset!(hirpdag_test_configs_one, $($items)*);
    };
}
