// Shared benchmark support.
//
// `hirpdag_bench_configs!` expands the given items (the `#[hirpdag]` type
// definitions and the benchmark implementation) once per hash-consing
// configuration preset, each in a `#[hirpdag_module]` module named after
// the preset. `bench_each_config!` registers a criterion benchmark for a
// function from each of those modules; its module/label list must stay in
// sync with the preset list in `hirpdag_bench_configs!`.
//
// This module also provides an *allocation-size* measurement so the same
// benchmark bodies can be run under criterion both for wall-clock time and
// for the number of bytes allocated. See `AllocBytes` below.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

// -----------------------------------------------------------------------------
// Allocation tracking
// -----------------------------------------------------------------------------

/// Total number of bytes handed out by the allocator since the process
/// started. Monotonically increasing: deallocations do not decrement it, so
/// the difference between two reads is the number of bytes *allocated* in
/// between (allocation traffic), independent of how much was freed again.
static ALLOCATED: AtomicUsize = AtomicUsize::new(0);

/// A `GlobalAlloc` that forwards every request to the system allocator while
/// accumulating the number of bytes allocated into `ALLOCATED`.
///
/// The counter is updated with `Relaxed` ordering: we only ever need the
/// running total to be eventually visible on the thread that reads it around a
/// measurement, and the benchmarks join all worker threads before ending a
/// measurement, so no stronger ordering is required. Forwarding straight to
/// `System` (including for `realloc`) keeps the overhead to a single relaxed
/// atomic add per allocation, which is negligible for the wall-clock
/// benchmarks that share this binary.
pub struct TrackingAllocator;

unsafe impl GlobalAlloc for TrackingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc_zeroed(layout);
        if !ptr.is_null() {
            ALLOCATED.fetch_add(layout.size(), Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new_ptr = System.realloc(ptr, layout, new_size);
        // Only count growth as freshly allocated bytes; shrinking a block does
        // not allocate.
        if !new_ptr.is_null() && new_size > layout.size() {
            ALLOCATED.fetch_add(new_size - layout.size(), Ordering::Relaxed);
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
/// records how many bytes were allocated while a benchmark routine ran,
/// instead of how long it took.
///
/// Allocation sizes are deterministic for a given workload, so — unlike the
/// jittery latencies criterion is designed to smooth out — a memory benchmark
/// does not need many samples to converge. The memory benchmark groups are
/// therefore configured for the minimum number of runs (flat sampling, a tiny
/// measurement window, so each of criterion's ten samples is a single
/// invocation).
///
/// Note on interned/leaking presets: because hash-consing tables are process
/// globals, the *first* time a workload builds a given DAG it allocates the
/// nodes, but a preset that never frees them (e.g. the `leak_*` presets) will
/// find them already interned on later invocations and allocate far less. The
/// reported figure is thus "bytes allocated per invocation in steady state":
/// the full construction cost for reference-counted presets that free between
/// runs, and only the incremental/transient cost for presets that retain every
/// node.
pub struct AllocBytes;

impl criterion::measurement::Measurement for AllocBytes {
    type Intermediate = usize;
    type Value = usize;

    fn start(&self) -> Self::Intermediate {
        ALLOCATED.load(Ordering::Relaxed)
    }

    fn end(&self, start: Self::Intermediate) -> Self::Value {
        ALLOCATED.load(Ordering::Relaxed).saturating_sub(start)
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
