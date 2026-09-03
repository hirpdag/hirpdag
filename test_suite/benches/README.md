# Benchmark suite

Each benchmark is a small program built from `#[hirpdag]` types and run
across every hash-consing configuration preset. The shared
`hirpdag_bench_configs!` / `bench_each_config!` machinery in
[`support/mod.rs`](support/mod.rs) expands each program once per preset
(reference-counting implementation × table backend) so a single benchmark
compares all of them on identical work.

The benchmarks cover the dimensions that move hash-consing performance: the
topology of the DAG, the size of a node's non-reference data, how much
structural sharing exists, which operation is stressed, whether the work is
concurrent, and whether nodes are destroyed (RC churn) rather than only built
and held.

## Coverage

| Benchmark | Topology | Node size | Sharing | Operation stressed | Concurrency | RC churn |
|---|---|---|---|---|---|---|
| [`fibonacci`](fibonacci.rs) | diamond (two parents per node) | small | high | build | no | no |
| [`primes`](primes.rs) | factor graph (variable fanout) | small | high | build + memoized rewrite | yes | no |
| [`rewrite_chain`](rewrite_chain.rs) | linear chain | small | none across passes | repeated full-graph rewrite (K passes) | no | no |
| [`expr_substitution`](expr_substitution.rs) | balanced binary tree | small | high (cycling vars) | memoized rewrite | no | no |
| [`sparse_rewrite`](sparse_rewrite.rs) | balanced binary tree (`Option` children) | small | none (unique ids) | mostly-unchanged rewrite (swept change fraction, K passes) | no | no |
| [`large_nodes`](large_nodes.rs) | balanced blob tree | large (`Vec<u8>` payloads) | swept (unique-blob pool) | build + rewrite | no | no |
| [`churn`](churn.rs) | forest of small unique units | small | none (units share nothing) | build + drop | no | yes |
| [`builder_edits`](builder_edits.rs) | balanced tree | small | high (persistent versions) | builder / persistent path-copy edit | no | no |
| [`serde_roundtrip`](serde_roundtrip.rs) | Fibonacci DAG | small | high | serialize + deserialize round trip | no | no |

## What each benchmark targets

- `fibonacci` builds a diamond DAG where every node has two parents, so
  structural sharing collapses an O(2^N) conceptual tree to N+1 unique nodes.
  Measures build and table-lookup cost under high sharing.

- `primes` builds a factor graph with variable fanout and rewrites it through
  a memoizing driver, single-threaded and across 2/4/8 threads (with identical
  or distinct per-thread data) to exercise the concurrent table backends.

- `rewrite_chain` isolates the rewrite infrastructure: K sequential rewrites
  over an N-node chain, each producing a fresh set of interned nodes, so the
  hashcons table cannot short-circuit and cost scales with K·N.

- `expr_substitution` builds an expression DAG where repeated leaf variables
  deduplicate identical subtrees, then runs a memoized substitution rewrite
  that visits each unique node exactly once. Measures structural sharing and
  memoization together.

- `sparse_rewrite` is the counterpart to the full-graph rewrites above
  (`rewrite_chain`, `primes`, `large_nodes` and `expr_substitution` all change
  every node). Here a rewrite touches only a fraction of the nodes, so most
  subtrees are structurally unchanged. Because the tree's children are
  `Option<TreeNode>` (no `Vec`), an unchanged subtree hits the
  `default_rewrite` fast path, which clones the input reference instead of
  rebuilding and re-interning it, with no allocation and no table lookup. The
  `change_period` parameter sweeps from a full-graph rewrite (`1`, every node
  changes) through a sparse one (`16`) to a pure no-op (`0`, nothing changes);
  the cost drops sharply as fewer nodes change, isolating the value of reusing
  input references.

- `large_nodes` moves the cost into large inline payloads: leaf nodes carry
  `Vec<u8>` blobs, so interning is dominated by hashing and comparing big data
  rather than by reference counting. Sweeps payload size and the sharing ratio.

- `churn` covers the destruction half of the lifecycle that the
  build-and-hold benchmarks never reach. A rolling window of unique units is
  continuously created and dropped, stressing reference-count decrements,
  `RC == 0` frees, and weak-table eviction. This is where `Leak` (never
  frees), `Arc`, the separated-count layouts, and the deferred-decrement `Tlc`
  separate from one another.

- `builder_edits` is the only benchmark exercising the builder API. Repeated
  persistent single-leaf edits rebuild one root-to-leaf path with
  `to_builder()` while every prior version stays alive, measuring builder and
  intern cost along a path plus the O(depth) allocation of path-copying.

- `serde_roundtrip` runs a full build, serialize and deserialize round trip
  over a highly shared DAG, in both postcard binary and JSON. Exercises the
  DAG-aware node-table dedup on write and re-interning through the hashcons
  table on read.

## Measurements

Each benchmark binary registers two criterion groups.

- `benches_time` (`Fibonacci`, `Churn`, ...) measures wall-clock time, with the
  sample counts criterion needs to smooth out latency jitter.

- `benches_mem` (`FibonacciMem`, ...) measures the *peak heap size* reached
  while the routine ran: the high-water mark of live bytes (allocated minus
  freed), tracked by the `AllocBytes` criterion measurement in
  [`support/mod.rs`](support/mod.rs) over a global allocator that forwards to
  the system one. Allocation sizes are deterministic for a given workload, so
  these groups run the minimum criterion allows (flat sampling, a nanosecond
  warm-up and measurement window, so each of the ten samples is a single
  invocation) and disable plots, which cannot be drawn from zero-variance
  samples. The approach follows
  [this gist](https://gist.github.com/DerSaidin/af295f89c047a049e4fc3193f520f12c).

  A peak is only meaningful if the run starts from an empty interning table,
  otherwise a preset that retains nodes across runs (the `leak_*` presets)
  finds them already interned and appears to allocate almost nothing. This is
  what the `reset-tables` feature is for: it generates a
  `hirpdag_reset_tables()` that empties each type's table in place through the
  table's existing lock, and `bench_each_config_mem!` calls it in an
  `iter_batched` setup step outside the measurement. The timing groups do not
  use it, so they are unaffected.

  `hirpdag_test_suite` enables `reset-tables` by default and it forwards to the
  same opt-in feature in `hirpdag` / `hirpdag_hashconsing`, so plain
  `cargo bench` measures memory correctly with nothing extra passed. Keep it
  on: `bench_each_config_mem!` calls `hirpdag_reset_tables()` unconditionally,
  so `--no-default-features` fails to compile the benchmarks rather than
  silently reporting misleading peaks.

  Not every benchmark has a memory group yet; see the repository `TODO.md`.

## Running

```sh
# All benchmarks, all default presets.
cargo bench -p hirpdag_test_suite

# One benchmark.
cargo bench -p hirpdag_test_suite --bench churn

# Filter to a preset or parameter set (criterion regex over the id).
cargo bench -p hirpdag_test_suite --bench churn -- LeakHashLinear

# Include the third-party concurrent-table backends.
cargo bench -p hirpdag_test_suite --features third-party-tables
```
