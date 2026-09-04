# Benchmarking

The benchmarks live in `test_suite/benches/` and run under
[criterion](https://crates.io/crates/criterion):

```
$ cargo bench                                  # every benchmark
$ cargo bench --bench primes                   # one benchmark
$ cargo bench --bench primes -- Parallel=8     # one benchmark, filtered by id
```

Every benchmark is compiled once per hash-consing configuration preset (the
`preset = "..."` values `#[hirpdag_module]` accepts), so the same workload can be
compared across reference-counting and table implementations. `cargo bench
--all-features` adds the presets built on third-party collection crates
(`arc_dashmap`, `arc_flurry`, `arc_skipmap`, `arc_arcswap`,
`arc_tovweaktable`).

## What a run measures

Timing every benchmark in every preset takes about an hour, which is too slow to
run while working on a change. A default run therefore covers:

| Group | Presets | Parameter sets |
| --- | --- | --- |
| Timed, `primes` | every preset | the key ones |
| Timed, every other benchmark | `arc_hash_linear`, `leak_hash_linear` | the key ones |
| Memory (`*Mem`) | every preset | all of them |

`primes` is the config-sweep benchmark: it is the one that always compares every
preset against every other, and it is where the published charts come from. The
other timed groups measure the two presets that bound the interesting axis --
`arc_hash_linear` (the default preset: atomic reference counting, weak entries
evicted from the table) and `leak_hash_linear` (the same table with no reference
counting and no frees at all) -- so the gap between them is what the node
lifecycle costs on that workload.

The memory groups cover everything because peak heap is deterministic: each is
ten single-iteration samples, so the whole preset list is nearly free to
measure.

Each benchmark's "key" parameter sets are the first entries of its `CONFIGS`
array; the rest sweep a workload axis (an extra thread count, a payload size, a
sharing ratio) and are there for a full run.

## Measuring more, or less

`HIRPDAG_BENCH_SCOPE` selects which presets a run measures:

```
$ HIRPDAG_BENCH_SCOPE=all cargo bench          # every preset, every parameter set
$ HIRPDAG_BENCH_SCOPE=key cargo bench          # the two key presets, everywhere
$ HIRPDAG_BENCH_SCOPE=arc_hash_linear,arc_dashmap cargo bench --all-features
```

The list accepts either the preset name (`arc_hash_linear`) or the label
criterion reports (`ArcHashLinear`); an unknown name is an error rather than a
silently empty run. Every benchmark binary prints the scope it is running with
on startup.

Criterion's own flags still apply on top, so a run whose numbers are going to be
published can ask for a longer measurement than the default 5s window. Filter to
the timed groups when doing so: criterion applies its flags to every group in
the binary, and in a memory group the "time" budget is interpreted in the
measurement's own unit -- bytes -- so a 15-second budget there asks for 15
billion bytes' worth of iterations.

```
$ HIRPDAG_BENCH_SCOPE=all cargo bench --bench primes -- \
      --measurement-time 15 --warm-up-time 3 'Primes2000/'
```

The trailing argument is criterion's benchmark-id filter; `Primes2000/` matches
the timed group but not `Primes2000Mem/`.

## Memory benchmarks

`support::AllocBytes` is a criterion measurement that reports the *peak heap
size* a run reached: a global allocator tracks live bytes (allocations minus
deallocations) and the measurement reports the high-water mark. The `*Mem`
groups use it in place of wall-clock time, so a memory benchmark reads as
`time: [49.275 KiB ...]` -- criterion's label, the value is bytes.

Each measured invocation is preceded by `hirpdag_reset_tables()` outside the
measurement, so every run starts from an empty interning table; without that, a
preset that retains nodes across runs would find them already interned and
appear to allocate nothing. That reset needs the `reset-tables` feature, which
is on by default for the test suite.

## Benchmarks

| Benchmark | What it exercises |
| --- | --- |
| `primes` | Node creation with shared sub-DAGs, single- and multi-threaded. The config sweep. |
| `fibonacci` | A diamond DAG where every node has two parents: intern-heavy, maximal sharing. |
| `expr_substitution` | A memoized rewrite over an expression DAG, at two sharing ratios. |
| `rewrite_chain` | Repeated full-graph rewrites, where no rewrite result is already interned. |
| `sparse_rewrite` | The unchanged-node fast path in `default_rewrite`, swept from full to no-op. |
| `large_nodes` | Large (`Vec<u8>`) payloads: hashing and equality dominate, and dedup is where memory is won. |
| `builder_edits` | Persistent path-copying edits through `to_builder()`, retaining every version. |
| `churn` | Create and drop with a bounded live window: reference-count decrements, frees, table eviction. |
| `serde_roundtrip` | DAG-aware serialize/deserialize (postcard and JSON), including re-interning on the way in. |

## Comparing runs

Criterion writes results to `target/criterion/` and compares each run against
the previous one in the same directory. To compare deliberately, name a
baseline:

```
$ git checkout main && cargo bench -- --save-baseline main
$ git checkout my-change && cargo bench -- --baseline main
```

The violin plots in `docs/benchmark_results/` (shown in the README) are copied
out of `target/criterion/` by `docs/update_benchmark_results.sh` after a
full-scope `primes` run.

There is no tooling yet for collecting results across many revisions; see
`TODO.md`.
