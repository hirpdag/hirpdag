// Shared benchmark support.
//
// `hirpdag_each_config!` holds the list of hash-consing configuration presets.
// `hirpdag_bench_configs!` drives it to expand the given items (the `#[hirpdag]`
// type definitions and the benchmark implementation) once per preset, each in a
// `#[hirpdag_module]` module named after it, and the `bench_*` macros drive it
// to register a criterion benchmark for a function from each of those modules.
//
// Every benchmark is compiled for every preset, but a run does not measure
// every one of them: timing a benchmark parameter set costs seconds, so
//
//   * the timed groups measure `KEY_CONFIGS` (two presets),
//   * except `primes`, the config-sweep benchmark, which times every preset,
//   * and the memory groups measure every preset, because a peak-heap
//     measurement is deterministic and takes one iteration per sample.
//
// `HIRPDAG_BENCH_SCOPE` overrides that: `all` measures every preset (and every
// parameter set) in every group, `key` narrows every group to `KEY_CONFIGS`,
// and a comma-separated preset list measures exactly those. See `Coverage`,
// `config_enabled` and `time_params` below, and `docs/BENCHMARKING.md`.
//
// This module also provides an *allocation-size* measurement so the same
// benchmark bodies can be run under criterion both for wall-clock time and
// for the number of bytes allocated. See `AllocBytes` below.

// Every bench binary compiles this whole module, but no single benchmark uses
// all of it: only the config-sweep benchmark registers with
// `bench_all_configs!`, and only benchmarks with more parameter sets than the
// timed groups run call `time_params`.
#![allow(dead_code, unused_macros)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

// -----------------------------------------------------------------------------
// Allocation tracking
// -----------------------------------------------------------------------------

/// Bytes currently live (allocated but not yet freed): every allocation adds
/// its size, every deallocation subtracts it. This is the running heap size.
static LIVE: AtomicUsize = AtomicUsize::new(0);

/// High-water mark of [`LIVE`] since it was last reset (see
/// [`AllocBytes::start`]). This is what the memory benchmark reports: the peak
/// heap size reached while a workload ran.
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Update `PEAK` to be at least `live`. `fetch_max` makes this correct even
/// when several worker threads allocate concurrently.
#[inline]
fn observe_peak(live: usize) {
    PEAK.fetch_max(live, Ordering::Relaxed);
}

/// A `GlobalAlloc` that forwards every request to the system allocator while
/// tracking the live heap size and its peak.
///
/// Allocations add to `LIVE` and push `PEAK` up; deallocations subtract from
/// `LIVE`. The counters use `Relaxed` ordering: the benchmarks join all worker
/// threads before ending a measurement, so the peak is fully visible to the
/// reader by then, and no stronger ordering is required. Forwarding straight to
/// `System` (including for `realloc`) keeps the overhead to a couple of relaxed
/// atomics per allocation, negligible for the wall-clock benchmarks that share
/// this binary.
pub struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            observe_peak(live);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            let live = LIVE.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            observe_peak(live);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        if !new_ptr.is_null() {
            let old_size = layout.size();
            if new_size >= old_size {
                let live =
                    LIVE.fetch_add(new_size - old_size, Ordering::Relaxed) + (new_size - old_size);
                observe_peak(live);
            } else {
                LIVE.fetch_sub(old_size - new_size, Ordering::Relaxed);
            }
        }
        new_ptr
    }
}

/// Install the tracking allocator as the global allocator for the benchmark
/// binary. There is exactly one `#[global_allocator]` per binary, and every
/// bench file includes this module, so this covers both the time and the
/// memory benchmark groups.
#[global_allocator]
static GLOBAL: TrackingAllocator = TrackingAllocator;

// -----------------------------------------------------------------------------
// Criterion measurement: bytes allocated
// -----------------------------------------------------------------------------

/// A criterion [`Measurement`](criterion::measurement::Measurement) that
/// records the *peak heap size* reached while a benchmark routine ran (the
/// high-water mark of live bytes = sum of allocations minus deallocations),
/// instead of how long it took.
///
/// Allocation sizes are deterministic for a given workload, so a memory
/// benchmark does not need the many samples criterion uses to smooth out
/// jittery latencies. The memory benchmark groups are
/// therefore configured for the minimum number of runs (flat sampling, a tiny
/// measurement window, so each of criterion's ten samples is a single
/// invocation).
///
/// The reported figure is the peak *increase* in live heap during the run,
/// relative to the heap size at [`start`](Self::start). For this to equal the
/// cost of building the DAG from scratch, the run must start from an empty
/// hash-consing table; otherwise a preset that retains nodes across runs
/// (e.g. the `leak_*` presets) finds them already interned and allocates
/// little. See [`crate::support`] docs / the bench setup for how each measured
/// run is given a fresh table.
pub struct AllocBytes;

impl criterion::measurement::Measurement for AllocBytes {
    type Intermediate = usize;
    type Value = usize;

    fn start(&self) -> Self::Intermediate {
        // Reset the peak to the current live size so the measurement captures
        // only the growth caused by this run. Criterion runs measurements
        // sequentially, so there is no concurrent measurement to race with.
        let base = LIVE.load(Ordering::Relaxed);
        PEAK.store(base, Ordering::Relaxed);
        base
    }

    fn end(&self, start: Self::Intermediate) -> Self::Value {
        PEAK.load(Ordering::Relaxed).saturating_sub(start)
    }

    fn add(&self, v1: &Self::Value, v2: &Self::Value) -> Self::Value {
        v1 + v2
    }

    fn zero(&self) -> Self::Value {
        0
    }

    fn to_f64(&self, value: &Self::Value) -> f64 {
        *value as f64
    }

    fn formatter(&self) -> &dyn criterion::measurement::ValueFormatter {
        &AllocBytesFormatter
    }
}

/// Formats allocation-size values using binary (IEC) byte units.
struct AllocBytesFormatter;

impl AllocBytesFormatter {
    fn scale(typical: f64, values: &mut [f64]) -> &'static str {
        let (factor, unit) = if typical < 1024.0 {
            (1.0, "B")
        } else if typical < 1024.0 * 1024.0 {
            (1.0 / 1024.0, "KiB")
        } else if typical < 1024.0 * 1024.0 * 1024.0 {
            (1.0 / (1024.0 * 1024.0), "MiB")
        } else {
            (1.0 / (1024.0 * 1024.0 * 1024.0), "GiB")
        };
        for val in values.iter_mut() {
            *val *= factor;
        }
        unit
    }
}

impl criterion::measurement::ValueFormatter for AllocBytesFormatter {
    fn scale_values(&self, typical_value: f64, values: &mut [f64]) -> &'static str {
        Self::scale(typical_value, values)
    }

    fn scale_throughputs(
        &self,
        typical_value: f64,
        _throughput: &criterion::Throughput,
        values: &mut [f64],
    ) -> &'static str {
        // The benchmarks do not set a throughput, so this is not exercised;
        // fall back to plain byte scaling rather than a bytes-per-second unit.
        Self::scale(typical_value, values)
    }

    fn scale_for_machines(&self, _values: &mut [f64]) -> &'static str {
        // Raw bytes, unscaled, for the CSV/machine-readable output.
        "B"
    }
}

// -----------------------------------------------------------------------------
// Which configurations a run measures
// -----------------------------------------------------------------------------

/// The presets every benchmark is *compiled* for, and which any run may
/// therefore select. Must stay in sync with the module lists in
/// `hirpdag_bench_configs!` and `hirpdag_each_config!`.
pub const CORE_CONFIGS: &[&str] = &[
    "arc_hash_linear",
    "arc_hash_sorted",
    "leak_hash_linear",
    "sep_hash_linear",
    "seppad_hash_linear",
    "sepu32_hash_linear",
    "tlc_hash_linear",
];

/// Presets compiled only with the `third-party-tables` feature.
pub const THIRD_PARTY_CONFIGS: &[&str] = &[
    "arc_tovweaktable",
    "arc_dashmap",
    "arc_flurry",
    "arc_skipmap",
    "arc_arcswap",
];

/// Whether [`THIRD_PARTY_CONFIGS`] are compiled into this binary.
pub const THIRD_PARTY_COMPILED: bool = cfg!(feature = "third-party-tables");

/// The presets a *timed* benchmark measures by default.
///
/// Timing one benchmark parameter set takes seconds, so running every preset
/// for every benchmark costs the better part of an hour. These two are the pair
/// that bounds the interesting axis: `arc_hash_linear` is the default preset
/// (atomic reference counting, weak entries evicted from the table), and
/// `leak_hash_linear` is the same table with no reference counting and no frees
/// at all, so the gap between them is what the lifecycle machinery costs. The
/// remaining presets vary within that gap, and a run that wants them says so
/// (see [`Coverage`] and `HIRPDAG_BENCH_SCOPE`).
pub const KEY_CONFIGS: &[&str] = &["arc_hash_linear", "leak_hash_linear"];

/// How much of the preset list a benchmark group covers when the run does not
/// ask for something specific.
#[derive(Copy, Clone, PartialEq, Eq)]
pub enum Coverage {
    /// [`KEY_CONFIGS`] only. What the timed groups use, because their cost is
    /// linear in the number of presets.
    Key,
    /// Every compiled-in preset. What the memory groups use (a peak-heap
    /// measurement is deterministic, so it is ten single-iteration samples --
    /// cheap enough to cover the whole list), and what the designated
    /// config-sweep benchmark uses for timing too.
    Full,
}

/// Environment variable selecting which presets a run measures.
pub const SCOPE_ENV: &str = "HIRPDAG_BENCH_SCOPE";

/// Parsed value of [`SCOPE_ENV`].
enum Scope {
    /// Unset: each group measures what its [`Coverage`] asks for.
    Default,
    /// `key`: [`KEY_CONFIGS`] everywhere, including the groups that would
    /// otherwise cover every preset.
    Key,
    /// `all`: every preset everywhere, and every benchmark parameter set.
    All,
    /// A comma-separated preset list: exactly those, everywhere. Holds the
    /// names as written; matching normalizes both sides.
    Only(Vec<String>),
}

/// Compare preset names ignoring case and `_`, so both the preset name
/// (`arc_hash_linear`) and the label criterion reports (`ArcHashLinear`) are
/// accepted in [`SCOPE_ENV`].
fn normalize(name: &str) -> String {
    name.chars()
        .filter(|c| *c != '_')
        .flat_map(char::to_lowercase)
        .collect()
}

fn scope() -> &'static Scope {
    static SCOPE: std::sync::OnceLock<Scope> = std::sync::OnceLock::new();
    SCOPE.get_or_init(|| {
        let scope = parse_scope();
        describe_scope(&scope);
        scope
    })
}

fn parse_scope() -> Scope {
    let raw = match std::env::var(SCOPE_ENV) {
        Ok(raw) => raw,
        Err(_) => return Scope::Default,
    };
    let raw = raw.trim();
    if raw.is_empty() || raw.eq_ignore_ascii_case("default") {
        return Scope::Default;
    }
    if raw.eq_ignore_ascii_case("key") {
        return Scope::Key;
    }
    if raw.eq_ignore_ascii_case("all") {
        return Scope::All;
    }
    let names: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect();
    assert!(!names.is_empty(), "{}: no presets named", SCOPE_ENV);
    // A typo here would silently measure nothing, so reject unknown names
    // rather than running an empty benchmark suite.
    for name in &names {
        if CORE_CONFIGS.iter().any(|c| normalize(c) == normalize(name)) {
            continue;
        }
        if THIRD_PARTY_CONFIGS
            .iter()
            .any(|c| normalize(c) == normalize(name))
        {
            assert!(
                THIRD_PARTY_COMPILED,
                "{}: preset `{}` needs `--features third-party-tables`",
                SCOPE_ENV, name
            );
            continue;
        }
        panic!(
            "{}: unknown preset `{}`; known presets: {}",
            SCOPE_ENV,
            name,
            CORE_CONFIGS
                .iter()
                .chain(THIRD_PARTY_CONFIGS)
                .copied()
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    Scope::Only(names)
}

/// Print, once per benchmark binary, which presets this run measures. Without
/// this a trimmed default run looks like benchmarks silently went missing.
fn describe_scope(scope: &Scope) {
    let what = match scope {
        Scope::Default => format!(
            "timed groups: {}; memory groups and the config sweep: every preset",
            KEY_CONFIGS.join(", ")
        ),
        Scope::Key => format!("every group: {}", KEY_CONFIGS.join(", ")),
        Scope::All => "every group: every preset, every parameter set".to_string(),
        Scope::Only(names) => format!("every group: {}", names.join(", ")),
    };
    eprintln!("hirpdag benches: {what} (set {SCOPE_ENV}=all|key|<preset,...> to change)");
}

/// Whether `preset` is measured by a group with this `coverage`.
pub fn config_enabled(preset: &str, coverage: Coverage) -> bool {
    match scope() {
        Scope::Default => coverage == Coverage::Full || KEY_CONFIGS.contains(&preset),
        Scope::Key => KEY_CONFIGS.contains(&preset),
        Scope::All => true,
        Scope::Only(names) => names.iter().any(|n| normalize(n) == normalize(preset)),
    }
}

/// The benchmark parameter sets a *timed* group runs: the first `key` entries
/// of `all`, or all of them when the run asked for every configuration. Order
/// each benchmark's parameter list so the representative sets come first.
///
/// Memory groups pass every parameter set instead: one iteration per sample
/// makes the extra sets nearly free.
pub fn time_params<T>(all: &[T], key: usize) -> &[T] {
    match scope() {
        Scope::All => all,
        _ => &all[..key.min(all.len())],
    }
}

// -----------------------------------------------------------------------------
// Criterion configuration
// -----------------------------------------------------------------------------

/// Configuration shared by the wall-clock groups.
///
/// These workloads run for milliseconds per iteration and are not latency
/// jittery, so a short window is enough; raise it for a run whose numbers are
/// going to be published, with
/// `cargo bench -- --measurement-time 15 --warm-up-time 3`.
pub fn time_criterion() -> criterion::Criterion {
    criterion::Criterion::default()
        .sample_size(10)
        .warm_up_time(core::time::Duration::from_secs(2))
        .measurement_time(core::time::Duration::from_secs(5))
}

/// Configuration shared by the memory (peak-heap) groups.
///
/// Allocation sizes are deterministic for a given workload, so this asks for
/// the minimum number of runs: flat sampling with a measurement window of
/// nothing, making each of criterion's ten samples a single invocation.
/// `without_plots()` because criterion cannot render a distribution from
/// zero-variance samples.
pub fn mem_criterion() -> criterion::Criterion<AllocBytes> {
    criterion::Criterion::default()
        .with_measurement(AllocBytes)
        .without_plots()
        .sample_size(10)
        .warm_up_time(core::time::Duration::from_nanos(1))
        .measurement_time(core::time::Duration::from_nanos(1))
}

// -----------------------------------------------------------------------------
// Per-configuration expansion and registration
// -----------------------------------------------------------------------------

/// Expands `$callback!(@one <module>, <label>, <preset>, $($args)*)` once per
/// configuration: the one place the preset list is written, driving both the
/// module expansion below and the benchmark registration macros. It must stay
/// in sync with `CORE_CONFIGS` / `THIRD_PARTY_CONFIGS` above, which is what
/// `HIRPDAG_BENCH_SCOPE` is validated against.
macro_rules! hirpdag_each_config {
    ($callback:ident, $($args:tt)*) => {
        $callback!(@one arc_hash_linear, "ArcHashLinear", "arc_hash_linear", $($args)*);
        $callback!(@one arc_hash_sorted, "ArcHashSorted", "arc_hash_sorted", $($args)*);
        $callback!(@one leak_hash_linear, "LeakHashLinear", "leak_hash_linear", $($args)*);
        $callback!(@one sep_hash_linear, "SepHashLinear", "sep_hash_linear", $($args)*);
        $callback!(@one seppad_hash_linear, "SepPadHashLinear", "seppad_hash_linear", $($args)*);
        $callback!(@one sepu32_hash_linear, "SepU32HashLinear", "sepu32_hash_linear", $($args)*);
        $callback!(@one tlc_hash_linear, "TlcHashLinear", "tlc_hash_linear", $($args)*);
        // Tables backed by third-party collection crates (feature-gated).
        #[cfg(feature = "third-party-tables")]
        $callback!(@one arc_tovweaktable, "ArcTovWeakTable", "arc_tovweaktable", $($args)*);
        #[cfg(feature = "third-party-tables")]
        $callback!(@one arc_dashmap, "ArcDashMap", "arc_dashmap", $($args)*);
        #[cfg(feature = "third-party-tables")]
        $callback!(@one arc_flurry, "ArcFlurry", "arc_flurry", $($args)*);
        #[cfg(feature = "third-party-tables")]
        $callback!(@one arc_skipmap, "ArcSkipMap", "arc_skipmap", $($args)*);
        #[cfg(feature = "third-party-tables")]
        $callback!(@one arc_arcswap, "ArcArcSwap", "arc_arcswap", $($args)*);
    };
}

/// Expands the given items once per configuration preset, each in a
/// `#[hirpdag_module]` module named after the preset. Drives
/// `hirpdag_each_config!` so the preset list is written once; the label that
/// list carries is only used on the registration side, and is ignored here.
macro_rules! hirpdag_bench_configs {
    (@one $module:ident, $label:literal, $preset:literal, $($items:item)*) => {
        #[hirpdag::hirpdag_module(preset = $preset)]
        mod $module {
            $($items)*
        }
    };
    ($($items:item)*) => {
        hirpdag_each_config!(hirpdag_bench_configs, $($items)*);
    };
}

/// Registers one (configuration, parameter set) benchmark, if the run measures
/// that configuration. The `time` form measures wall-clock time; the `mem` form
/// measures peak heap, and precedes each measured invocation with
/// `hirpdag_reset_tables()` in an `iter_batched` setup step (run *outside* the
/// measurement) so every build starts from an empty interning table. Without
/// that, presets which retain nodes across runs (`leak_*`) would find them
/// already interned from a previous invocation and appear to allocate almost
/// nothing.
macro_rules! bench_one_config {
    (@one $module:ident, $label:literal, $preset:literal, time,
     $coverage:expr, $group:expr, $params:expr, $function:ident) => {
        if crate::support::config_enabled($preset, $coverage) {
            $group.bench_with_input(
                criterion::BenchmarkId::new($label, $params),
                &$params,
                |b, params| b.iter(|| crate::$module::$function(std::hint::black_box(params))),
            );
        }
    };
    (@one $module:ident, $label:literal, $preset:literal, mem,
     $coverage:expr, $group:expr, $params:expr, $function:ident) => {
        if crate::support::config_enabled($preset, $coverage) {
            $group.bench_with_input(
                criterion::BenchmarkId::new($label, $params),
                &$params,
                |b, params| {
                    b.iter_batched(
                        || crate::$module::hirpdag_reset_tables(),
                        |_| crate::$module::$function(std::hint::black_box(params)),
                        criterion::BatchSize::PerIteration,
                    )
                },
            );
        }
    };
}

/// Times `$function` in the [`Coverage::Key`] presets: what most benchmarks
/// use, so that a default `cargo bench` stays minutes rather than an hour.
macro_rules! bench_each_config {
    ($group:expr, $params:expr, $function:ident) => {
        hirpdag_each_config!(
            bench_one_config,
            time,
            crate::support::Coverage::Key,
            $group,
            $params,
            $function
        );
    };
}

/// Times `$function` in *every* configuration. Reserved for the designated
/// config-sweep benchmark (`primes`), which exists to compare the hash-consing
/// implementations against each other; every other benchmark measures the key
/// presets and widens only on request.
macro_rules! bench_all_configs {
    ($group:expr, $params:expr, $function:ident) => {
        hirpdag_each_config!(
            bench_one_config,
            time,
            crate::support::Coverage::Full,
            $group,
            $params,
            $function
        );
    };
}

/// Measures peak heap for `$function` in every configuration: unlike timing,
/// this is ten single-iteration samples of a deterministic quantity, so full
/// coverage is cheap and every benchmark gets it.
macro_rules! bench_each_config_mem {
    ($group:expr, $params:expr, $function:ident) => {
        hirpdag_each_config!(
            bench_one_config,
            mem,
            crate::support::Coverage::Full,
            $group,
            $params,
            $function
        );
    };
}
