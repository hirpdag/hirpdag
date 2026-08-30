# TODO

### Hashconsing optimization experiments

- [P1] Experiment with a single std::any::Any based map for Hashconsing and Rewrite caches.
  - Currently generate a separate map for each type, because this was the easiest thing to do.
  - Hash tables need some free space overhead to operate efficiently.
    Combining all of these hash tables into one may need less overall empty space overhead.

- [P1] Experiment with doing deallocation work on another thread when RC==0.

- [P2] Make memory allocation for hirpdag objects contiguous.
  - Can this be done by using https://github.com/rkyv/rkyv to help serialization?

- [P2] Optimize for small objects:
  - Just pack data into the handle type if less than 128bytes.
  - Warning if Hirpdag attribute is added to a struct which seems too small to benefit.

- [P2] Make hirpdag objects use read-only optimized datastructures.
  - e.g. If object contains a hashmap make it use perfect hashing (e.g https://lib.rs/crates/phf)
  - Use flat datastructures
  - Should Hirpdag automatically change a field to a more optimized type for you? Or just warn or something?

- [P2] Caching for `ref.hirpdag_compute_meta()`. e.g. `ref.hirpdag_compute_meta(meta_cache)`.

### Memoization cache improvements

Improvements to `HirpdagMemoizeMap` / the generated `HirpdagMemoizeCache`
(`hirpdag/src/base/memoize.rs`, `hirpdag_derive/src/lib.rs`).

- [P1] Bound what the cache keeps alive.
  - Keys and values are strong node references, so a long-lived cache pins every
    node it has ever seen; `clear()` is currently the only way to release them.
  - Options to try: weak keys/values with a purge pass (like
    `table/weak_entry.rs` and the `PURGE_LEN_MIN` purge in
    `table/hashmap_fallback_threadunsafe.rs`), a capacity bound with LRU or
    random eviction, or generational tables dropped between passes.
  - Evicting is always safe for correctness (the value can be recomputed), so
    the trade-off is recompute cost vs. retained memory; measure it.

- [P1] Avoid re-hashing on the `get_or_else` miss path.
  - A miss hashes the key twice for shard selection (`get`, then `get_shard`)
    plus twice more inside the shard's `HashMap`, and takes the shard lock
    twice.
  - Compute the hash once and reuse it for both the shard index and the map
    lookup (hashbrown's raw entry API, or a `HashMap` keyed by precomputed
    hash).

- [P1] Use a cheaper hasher than SipHash for node keys.
  - `DefaultHasher` is the default `BuildHasher` here, but node keys hash by
    interned identity (a pointer / creation id), which is already well
    distributed; an identity or multiply-shift hasher should be much cheaper.
  - The hash-consing tables have the same question; keep the answer shared
    rather than picking one per table.

- [P1] Share the sharding with the hash-consing tables.
  - `const N_SHARDS: usize = 8` and the mask-based shard selection are
    duplicated between `base/memoize.rs` and `table/shared_sharded.rs`, and 8
    is a guess unrelated to the machine or the workload.
  - Factor out one sharded-map helper, and make the shard count configurable
    (module config, or derived from `available_parallelism()`); sweep it in a
    benchmark.

- [P2] Let concurrent callers share one in-flight computation.
  - Threads racing to the same key today all compute it and the first result
    to land wins; this is correct but wasteful for expensive rules.
  - A per-key in-progress marker that later callers wait on would fix it, but
    must not deadlock the recursive case (`get_or_else` re-entering the same
    table for a child node is the normal traversal pattern), so it needs
    reentrancy detection; check that it is worth the complexity first.

- [P2] Allow values that are not the key type.
  - `HirpdagMemoize<T>` hard-codes `HirpdagMemoizeMap<T, T>`, so the generated
    cache only holds node -> node results; an analysis pass has to build its
    own map outside the cache even though `HirpdagMemoizeMap` supports any `V`.
  - Generalise the trait (e.g. `HirpdagMemoize<T, V = T>`) so a cache can host
    analysis results too.

- [P2] Finer-grained cache management.
  - `clear()` empties every type's table; add per-type clear, and consider
    keeping allocated capacity across passes (or `shrink_to_fit` on demand)
    so repeated rewrites do not rebuild the tables from scratch.
  - `len()` locks every shard, and `is_empty()` goes through `len()` instead of
    stopping at the first non-empty shard.

- [P2] Optional hit/miss instrumentation.
  - Feature-gated counters per table (hits, misses, races lost, evictions) so
    benchmarks and users can tell whether memoizing is paying for itself.

- [P2] See also the `std::any::Any` single-map experiment above, which covers
  the rewrite caches as well as the hash-consing tables.

- [P2] Benchmark the cache itself.
  - The existing benchmarks exercise memoization only indirectly through
    rewriting; none measures hit-path cost, shard contention across threads, or
    the shard-count/hasher/eviction trade-offs above.

### More benchmarks

- [P1] Perf measuring cache-misses, branch-misses, etc. instead of only execution time.

### Code cleanup

- [P1] General refactoring and cleanup:
  - Try to reduce use of generic and builders for hashconsing implementations.

### Features

- [P1] Visitor traversal code

- [P2] Warning if Hirpdag is used in a probably-wrong way
  - Adding the hirpdag attribute to struct which only contains 1 field and it is a hirpdag ref.

- [P3] IPC (hirpdag objects in shared memory, used from multiple processes)
  - Maybe special support for producer/consumer pattern? - only one process creates hirpdag objects

## Completed

### Hashconsing optimization experiments

- ~~[P0] Optimize rewrite code:~~
  - ~~If no changes we can copy input reference instead of hashconsing to reproduce it.~~
  - ~~Minimize reference count operations.~~
  - DONE: The macro-generated `default_rewrite` now rewrites each field into a
    local, compares the results against the originals (child `HirpdagRef` fields
    compare by pointer, leaf fields by value) and, when nothing changed, returns
    `self.clone()`, a single reference-count bump on the already-interned node.
    Previously every rewrite reconstructed the struct and went through
    `Self::new` (normalization + a hash-cons table lookup) even when the result
    was identical. That rebuild cloned every child field into a temporary struct
    (N ref-count increments), interned it, then dropped the temporary (N
    decrements). The unchanged fast path now costs one increment instead, so
    identity/partial rewrites that leave subtrees untouched no longer touch the
    hash-cons table or churn child reference counts. See
    `tests/base.rs::identity_rewrite_preserves_nodes` and
    `partial_rewrite_preserves_untouched_subtree`.

- ~~[P0] Optimize HirpdagRef implemetation of Ord:~~
  - ~~Should not need to do a deep cmp.~~
  - ~~Consider creation timestamp.~~
  - DONE: `HirpdagStorage` now carries a `hirpdag_creation_id: u64` (assigned from a global
    atomic counter at interning time). `HirpdagRef::cmp` uses pointer equality (O(1) fast
    path) then falls back to comparing creation IDs (O(1)). The previous deep structural
    comparison is still available as `hirpdag_cmp_deep()`.

### Experiment with more refcounting implementations

- ~~[P1] Locate many reference counts contiguously, separate from the data.~~
  - ~~Cachlines holding data can be shared read only and never dirtied (to other CPU cores)
    due to ref count updates.~~
  - https://users.rust-lang.org/t/why-does-arc-use-one-contiguous-allocation-for-data-and-counters/113319
  - https://ddanilov.me/shared-ptr-is-evil/
  - ~~Try padding ref counts to one (pair, strong and weak) per cacheline. Look for performance/space trade-off.~~
  - DONE: `reference/sepcount.rs` stores (strong, weak) count slots in a contiguous global
    arena (chunked, with a free list), separate from the data allocation. The handle is
    two pointers (data + slot). Three slot layouts explore the space/perf trade-off:
    `RefSep` (packed 16B slots), `RefSepPad` (one slot per 64B cacheline, no false
    sharing) and `RefSepU32` (packed 8B u32 slots, densest).

- ~~[P3] Thread local reference counts, periodically flush back to main counter.~~
  - ~~Is this even possible? Is it good?~~
  - DONE: `reference/tlc.rs` (`RefTlc`). Buffering *increments* thread-locally is unsound
    (a handle whose increment is still buffered can move to another thread whose drop
    takes the shared count to zero prematurely). Buffering *decrements* is safe: it only
    delays frees. Each thread keeps a `HashMap<addr, deferred_dec_count>`; drop buffers a
    decrement, clone/weak-upgrade first try to cancel against a buffered decrement (taking
    over the shared count the dropped handle held) before touching the shared atomic.
    The map is flushed after a bounded number of ops and at thread exit.

### More benchmarks

- ~~[P0] Benchmarks capture memory usage~~
  - ~~See https://gist.github.com/DerSaidin/af295f89c047a049e4fc3193f520f12c~~
  - ~~Should only need 1 or 2 runs because allocation sizes should be deterministic (compared to the jittery latency Criterion is designed to handle)~~
  - DONE: Each benchmark now has a `*Mem` criterion group measuring peak heap
    usage (high-water mark of live = allocated − freed bytes) via a custom
    `AllocBytes` criterion measurement over a tracking global allocator. The
    opt-in `reset-tables` feature empties each type's interning table between
    runs, in place through the table's existing lock, so the timing benchmarks
    are unaffected and every measured build starts from empty. This gives
    deterministic from-empty peaks even for the non-freeing `leak_*` presets.
    The memory groups use flat sampling with a minimal warm-up/measurement
    window (criterion's floor is 10 samples) since allocation sizes do not need
    the many samples that jittery latency does.

- ~~[P1] More benchmark programs.~~
  - ~~Benchmarks where node data is pretty large~~
  - DONE: Four new benchmark programs added, each broadening a dimension the
    existing suite did not cover (see `test_suite/benches/README.md` for the
    full coverage table across topology / node size / sharing / operation /
    concurrency / RC churn):
    * `large_nodes` gives nodes large `Vec<u8>` payloads (a content-addressed
      blob tree), so interning cost is dominated by hashing/comparing big data
      rather than reference counting; sweeps payload size and sharing ratio.
    * `churn` continuously creates and drops unique units through a rolling
      live-window, stressing reference-count decrements, `RC == 0` frees, and
      weak-table eviction (the destruction half of the lifecycle the
      build-and-hold benchmarks never reach; `leak_*` presets separate clearly).
    * `builder_edits` makes persistent single-leaf edits via `to_builder()`,
      path-copying one root-to-leaf path per edit while retaining every version.
    * `serde_roundtrip` runs a build, serialize and deserialize round trip over
      a highly shared Fibonacci DAG, in both postcard binary and JSON.

### Code cleanup

- ~~[P0] Builder API?~~
  - ~~Supporting modifications on an existing node.~~
  - ~~Avoid hashconsing construction until once at the end of the builder.~~
  - DONE: Each `#[hirpdag]` struct now generates a `FooBuilder` type. Use
    `Foo::builder()` to construct a new node with setter chaining, or
    `node.to_builder()` to derive a modified copy of an existing node.
    Both paths go through a single `build()` call that hashcons at the end.

- ~~[P1] Address current code duplication in benchmarks for running the same benchmark with different hashconsing implementations selected.~~
  
- ~~[P3] Find a better solution than the `#[hirpdag_end] pub struct HirpdagEndMarker;` hack.~~
  - DONE: `#[hirpdag_module]` on the module replaces `#[hirpdag]`/`#[hirpdag_end]`
    pairs and the global registry between them. See
    docs/adr/0002-module-attribute-macro.md.

### Features

- ~~[P0] Serialization/deserialization~~
  - DONE: DAG-aware serialization built on serde. `#[hirpdag_module]` generates
    `hirpdag_serialize`/`hirpdag_deserialize` (postcard binary) and
    `hirpdag_serialize_json`/`hirpdag_deserialize_json` (text). Each unique node
    is written exactly once into a topologically ordered node table with u64
    indices; deserialization re-interns through the hashcons table. See
    `docs/design/serialization.md` and `docs/adr/0001-serde-dag-aware-serialization.md`.
