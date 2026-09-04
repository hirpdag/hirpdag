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

- [P2] Reuse the computed hash inside the shard's `HashMap`.
  - Shard selection now hashes the key once per `get_or_else` call and reuses
    the shard for the lookup and the insert, but the shard's `HashMap` still
    hashes the key again for each of those two visits.
  - Passing the hash through needs hashbrown's raw entry API or a `HashMap`
    keyed by precomputed hash; measure whether it pays for the extra machinery
    before taking it on.
  - The two lock acquisitions are inherent: no lock may be held while `compute`
    runs, because a computation is free to recurse into the same shard.

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

- [P0] Add memory measurement groups (`benches_mem`) to remaining benchmarks.
  - `churn.rs`, `builder_edits.rs`, `large_nodes.rs`, and `serde_roundtrip.rs` currently only have wall-clock timing groups.
  - Add `bench_*_mem` using `AllocBytes` and `bench_each_config_mem!` so all 9 benchmarks track peak heap usage (especially critical for `churn` to track deallocation/eviction and `large_nodes` to measure memory savings across sharing ratios).

- [P1] Multi-threaded concurrency and contention benchmarks.
  - **Concurrent Churn / Dropping**: Multi-threaded node creation and drop to stress atomic decrement contention, `RefTlc` thread-local decrement flushing, and test whether `RefSepPad` eliminates cache-line false sharing compared to `RefSep`.
  - **Concurrent Read/Write Contention**: Mixed reader-writer workloads (concurrent lookup of existing interned nodes while writers insert new nodes) to evaluate `TableSharedDashMap`, `TableSharedFlurry`, `TableSharedSkipMap`, and `TableSharedArcSwap` against `TableSharedSharded`.
  - **Shared Memoization Cache Contention**: Multi-threaded memoized rewrite passes sharing a single `HirpdagRewriteMemoized` cache across threads (currently `primes.rs` allocates separate per-thread rewriters, avoiding cache contention).

- [P1] Benchmark additional reference types, table backends, and normalizers.
  - **`RefRc` preset**: Add a single-threaded `rc_hash_linear` preset / benchmark to quantify the atomic ref-counting overhead of `RefArc` on single-threaded workloads.
  - **`TableSharedMutex` vs `TableSharedSharded`**: Benchmark single coarse mutex table against sharded mutex table to measure the scalability and locking overhead of sharding.
  - **Normalizer overhead**: Benchmark `#[hirpdag(normalizer)]` and `Expr::spawn()` normalization during construction vs un-normalized construction.

- [P1] Perf measuring cache-misses, branch-misses, etc. instead of only execution time.
  - Use `perf_event_open` / `criterion-perf-events` (or `criterion-linux-perf`) to measure L1/LLC cache misses, branch mispredictions, and instructions per cycle.
  - Crucial for validating cache-line false sharing on `RefSep` vs `RefSepPad` and comparing linear search (`TableVecLinearWeak`) vs binary search (`TableVecSortedWeak`).

- [P2] Microbenchmarks for common DAG operations and traversals.
  - **Comparison and Hashing**: Compare $O(1)$ pointer/creation-ID comparison (`HirpdagRef::cmp`, `Eq`) and $O(1)$ hashing against $O(N)$ deep structural comparison (`hirpdag_cmp_deep`) in tight loops.
  - **Read-Only / Visitor Passes**: Benchmark pure traversal/visitor passes and metadata queries (`hirpdag_get_meta` vs recomputing `hirpdag_compute_meta`).
  - **Serialization Graph Variety**: Expand `serde_roundtrip` to cover wide trees with high fanout, large payload DAGs (`large_nodes`), and deep chains in addition to the Fibonacci DAG.

### Features

- [P1] Visitor traversal code

- [P2] Warning if Hirpdag is used in a probably-wrong way
  - Adding the hirpdag attribute to struct which only contains 1 field and it is a hirpdag ref.

- [P3] IPC (hirpdag objects in shared memory, used from multiple processes)
  - Maybe special support for producer/consumer pattern? - only one process creates hirpdag objects
