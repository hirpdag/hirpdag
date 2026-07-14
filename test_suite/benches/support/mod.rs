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
/// Allocation sizes are deterministic for a given workload, so — unlike the
/// jittery latencies criterion is designed to smooth out — a memory benchmark
/// does not need many samples to converge. The memory benchmark groups are
/// therefore configured for the minimum number of runs (flat sampling, a tiny
/// measurement window, so each of criterion's ten samples is a single
/// invocation).
///
/// The reported figure is the peak *increase* in live heap during the run,
/// relative to the heap size at [`start`](Self::start). For this to equal the
/// cost of building the DAG from scratch, the run must start from an empty
/// hash-consing table — otherwise a preset that retains nodes across runs
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

/// Like [`bench_each_config!`], but for the memory (peak-heap) measurement.
///
/// Each measured invocation is preceded by `hirpdag_reset_tables()` in an
/// `iter_batched` setup step (run *outside* the measurement) with
/// `BatchSize::PerIteration`, so every build starts from an empty interning
/// table. Without this, presets that retain nodes across runs (`leak_*`) would
/// find them already interned from a previous invocation and appear to allocate
/// almost nothing.
macro_rules! bench_each_config_mem {
    (@one $group:expr, $params:expr, $function:ident, $module:ident, $label:literal) => {
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
    };
    ($group:expr, $params:expr, $function:ident) => {
        bench_each_config_mem!(@one $group, $params, $function, arc_hash_linear, "ArcHashLinear");
        bench_each_config_mem!(@one $group, $params, $function, arc_hash_sorted, "ArcHashSorted");
        bench_each_config_mem!(@one $group, $params, $function, leak_hash_linear, "LeakHashLinear");
        bench_each_config_mem!(@one $group, $params, $function, sep_hash_linear, "SepHashLinear");
        bench_each_config_mem!(@one $group, $params, $function, seppad_hash_linear, "SepPadHashLinear");
        bench_each_config_mem!(@one $group, $params, $function, sepu32_hash_linear, "SepU32HashLinear");
        bench_each_config_mem!(@one $group, $params, $function, tlc_hash_linear, "TlcHashLinear");
        // Tables backed by third-party collection crates (feature-gated). These
        // backends do not yet implement table reset, so their memory figures
        // reflect incremental (not from-empty) allocation.
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_tovweaktable, "ArcTovWeakTable");
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_dashmap, "ArcDashMap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_flurry, "ArcFlurry");
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_skipmap, "ArcSkipMap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_arcswap, "ArcArcSwap");
        #[cfg(feature = "third-party-tables")]
        bench_each_config_mem!(@one $group, $params, $function, arc_evmap, "ArcEvmap");
    };
}
