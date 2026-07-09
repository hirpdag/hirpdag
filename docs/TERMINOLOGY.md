# HIRPDAG Terminology

This document defines the domain vocabulary used throughout the hirpdag codebase and book.

---

## Core Concepts

### HIRPDAG
**H**ash Consed · **I**mmutable · **R**eference Counted · **P**ersistent · **D**irected **A**cyclic **G**raph.

The acronym names both the project and the combination of properties that make the data structure useful.  Each property reinforces the others — see `book/src/ch02-00-hirpdag.md`.

### Hash Consing / Interning
The practice of deduplicating structurally identical nodes so that only one allocation ever exists for a given value.  When a new node is constructed, the table checks whether an equal node already exists; if so, the existing pointer is returned instead of allocating a new one.

Source: `hirpdag/src/base/reference.rs` — `HirpdagHashconsTable::hirpdag_hashcons`

### Pointer Equality
Because hash-consing guarantees at most one allocation per distinct value, two `HirpdagRef`s are equal **iff** they point to the same address — an O(1) check.  Deep structural comparison (`hirpdag_cmp_deep`) is available but O(n).

Source: `hirpdag/src/base/reference.rs` — `HirpdagRef` `PartialEq` impl

### Directed Acyclic Graph (DAG)
The shape of the data structure.  Nodes reference children but never form cycles.  Hash-consing means multiple parent nodes can share the same child allocation — this sharing is what gives DAGs their space efficiency over trees.

### Persistence
The ability to create a modified version of a node without mutating the original.  Because all nodes are immutable and hash-consed, "modifying" a node means constructing a new one — existing references to the old node remain valid.

### Referential Transparency
Nodes with identical structure always have identical meaning and share the same interned pointer.  Programs can compare, cache, or substitute any two equal nodes freely.

---

## Metadata

### `HirpdagMeta`
Aggregated structural metadata cached on every interned node.  Computed bottom-up at intern time; reading any field is O(1).

Source: `hirpdag/src/base/meta.rs`

### count (`HirpdagMetaCountType` = u32)
Total number of nodes in the subtree rooted at this node (saturating).  Useful for estimating expression size without traversal.

### height (`HirpdagMetaHeightType` = u16)
Distance from this node to its deepest leaf (saturating).  Proportional to the longest dependency chain.

### flags (`HirpdagMetaFlagType` = u16)
A user-defined bitfield propagated upward via bitwise OR.  Allows quickly testing whether *any* node in a subtree has a property (e.g. "contains a free variable") without traversal.

### `HirpdagComputeMeta`
Trait implemented by every field type.  The macro-generated implementation for each struct folds together the results from all fields.  Leaf types (numbers, strings) return zero; child `HirpdagRef` fields return their cached metadata.

Source: `hirpdag/src/base/meta.rs`

---

## References

### Strong Reference
An owning handle that keeps the allocation alive.  Implemented by `RefArc<D>` (thread-safe `Arc`), `RefRc<D>` (single-threaded `Rc`), and `RefLeak<D>` (never deallocates).

Trait: `hirpdag_hashconsing::Reference`

### Weak Reference
A non-owning handle that can be upgraded to a strong reference if one still exists, or fails if the allocation has been dropped.  The hash-consing table holds only weak references so that nodes are freed when no user holds a strong reference.

Trait: `hirpdag_hashconsing::ReferenceWeak`

### Creation ID
A monotonically increasing integer (u64) assigned to each node at intern time.  If node A was interned after node B (e.g. because A contains B as a child), then B's ID is strictly less than A's.  Used to give `HirpdagRef` a total O(1) ordering consistent with DAG dependency order.

Source: `hirpdag/src/base/reference.rs` — `HIRPDAG_CREATION_COUNTER`

---

## Tables

### `Table<D, R>`
The single-threaded storage unit for a hash-consing table.  Implementations differ in lookup strategy and memory layout.

Trait: `hirpdag_hashconsing::Table`

### `TableShared<D, R>`
The thread-safe hash-consing interface (`get` / `get_or_insert` over `&self`).  Implementations choose their concurrency strategy: some wrap one or more inner single-threaded `Table` instances behind locks, others store the mapping directly in a concurrent collection.  Note the trait is *not* parameterized over an inner `Table` — a backend that needs one (mutex/sharded) carries it as its own generic, so backends that don't (the concurrent-collection ones) name no table at all.

Trait: `hirpdag_hashconsing::TableShared`

### `WeakEntry`
The per-element storage unit inside vector-backed tables: a precomputed hash plus a weak reference.  The cached hash allows O(1) filtering before the equality check.

Source: `hirpdag_hashconsing/src/weak_entry.rs`

### `TableVecLinearWeak`
Unsorted `Vec` of weak entries; O(n) linear search.  Simple, allocation-friendly, suitable for small node sets.

### `TableVecSortedWeak`
Hash-sorted `Vec` of weak entries; O(log n) binary search to the hash run, then O(k) linear scan within the run.  Better than linear for medium node sets.

### `TableHashmapFallbackWeak`
`HashMap`-based table with a fallback to an alternate table for small sizes.  O(1) average lookup; the fallback handles the cold start efficiently.

### `TableSharedMutex`
`TableShared` implementation wrapping a single `Mutex`.  Simple; all threads serialise on one lock.

### `TableSharedSharded`
`TableShared` implementation using `N_SHARDS` (= 8) independent mutexes.  Threads hashing to different shards never contend.  Shard selection is `hash & (N_SHARDS - 1)` — a bitmask because `N_SHARDS` is a power of two.

Source: `hirpdag_hashconsing/src/tableshared_sharded.rs`

### Concurrent-collection `TableShared` backends
A family of `TableShared` implementations that store the interned mapping directly in a third-party concurrent collection instead of delegating to an inner `Table` (they are not generic over one). All store **strong** references, so unreferenced nodes are retained rather than garbage-collected, and all require a `Send + Sync` reference (they are wired to `RefArc`).  Selectable via the presets `arc_dashmap`, `arc_flurry`, `arc_skipmap`, `arc_arcswap`, `arc_evmap`.

| Type | Preset | Backend | Strategy |
| --- | --- | --- | --- |
| `TableSharedDashMap` | `arc_dashmap` | [`dashmap`] | Bucket-striped concurrent hash map; per-shard locks. |
| `TableSharedFlurry` | `arc_flurry` | [`flurry`] | Lock-free hash map (Java `ConcurrentHashMap` port); keys must be `Ord`. |
| `TableSharedSkipMap` | `arc_skipmap` | [`crossbeam-skiplist`] | Lock-free ordered skip list; `O(log n)` lookup, no hasher. |
| `TableSharedArcSwap` | `arc_arcswap` | [`arc-swap`] | RCU / copy-on-write: lock-free reads, whole-map clone per insert (`O(n)` writes). |
| `TableSharedEvmap` | `arc_evmap` | [`evmap`] | Left-right double-buffering; values must be `Hash + Eq`. |

[`dashmap`]: https://crates.io/crates/dashmap
[`flurry`]: https://crates.io/crates/flurry
[`crossbeam-skiplist`]: https://crates.io/crates/crossbeam-skiplist
[`arc-swap`]: https://crates.io/crates/arc-swap
[`evmap`]: https://crates.io/crates/evmap

Source: `hirpdag_hashconsing/src/tableshared_dashmap.rs`, `tableshared_flurry.rs`, `tableshared_skipmap.rs`, `tableshared_arcswap.rs`, `tableshared_evmap.rs`

---

## Rewriting

### Rewriter
A user-defined struct that implements the generated `HirpdagRewriter` trait.  It has one method per `#[hirpdag]` type (`rewrite_Foo`, `rewrite_Bar`, …) and is the single entry point for tree transformations.

Generated by: `#[hirpdag_module]`

### `default_rewrite`
The default traversal provided for every `#[hirpdag]` type.  It rewrites each child field through the rewriter, then reconstructs the node with the new children (returning the original if nothing changed).  Override individual `rewrite_*` methods to intercept specific node types.  Public, so rewriters may be defined outside the hirpdag module (any fields they read must then be `pub`).

### Memoization
`HirpdagRewriteMemoized<Rewriter>` wraps any `HirpdagRewriter` and caches the result of each `rewrite_*` call.  Because nodes are hash-consed, the cache key is the creation ID — an O(1) lookup that avoids repeated traversal of shared subtrees.

Generated by: `#[hirpdag_module]`

### Normalization
A user-supplied transformation applied during construction (`new()`), before interning.  Used to enforce canonical forms such as sorting commutative operands, flattening nested associative operators, or folding constant subexpressions.  Only triggered when `#[hirpdag(normalizer)]` is present.

---

## Macro Interface

### `#[hirpdag_module]`
The attribute macro.  Applied to an inline module (`mod name { ... }`), it expands every struct and enum in the module marked `#[hirpdag]`, passes other items through unchanged, and generates the module-level code (the `HirpdagRewriter` trait with one `rewrite_*` method per type, `HirpdagRewriteMemoized<Rewriter>`, and the serialization machinery).  The hash-consing configuration is given as attribute arguments: a named `preset = "..."` or the explicit type overrides listed below.  The generated code refers to the hirpdag crate by absolute paths, so the module needs no imports beyond what the user's own code uses.  Must be an outer attribute on an inline module; Rust does not accept the inner form (`#![hirpdag_module]`, rust-lang/rust#54726).

### `#[hirpdag]`
Inert marker consumed by `#[hirpdag_module]`.  Applied to a named struct or enum inside the module to generate:
- An inner struct holding the fields.
- A `Builder` type for ergonomic construction.
- A global `lazy_static` hash-consing table.
- `spawn` (direct intern) and optionally `new` (intern via normalizer) constructors.
- `HirpdagStruct`, `HirpdagComputeMeta`, `HirpdagRewritable` impls.

### `#[hirpdag(normalizer)]`
Variant of `#[hirpdag]` that also generates a `new()` constructor calling the user's normalizer function before interning.

### `spawn`
Low-level node constructor that interns fields directly, bypassing any normalizer.  Use when you know the value is already in canonical form.

### `new`
High-level node constructor that runs the user-supplied normalizer before interning.  Only generated when `#[hirpdag(normalizer)]` is specified.

### `builder` / `to_builder` / `build`
A three-step ergonomic construction API.  `builder()` creates a fresh `Builder`; `to_builder()` copies an existing node into a builder (copy-on-write); `build()` finalises and interns the result.

---

## Configuration Overrides

The following `#[hirpdag_module(...)]` options override the default pluggable implementations:

| Option | Default | Controls |
|--------|---------|----------|
| `reference_type` | `RefArc` | Strong reference (e.g. swap in `RefRc` for single-threaded use) |
| `reference_weak_type` | `RefArcWeak` | Weak reference |
| `table_type` | `TableVecLinearWeak` | Inner table strategy |
| `tableshared_type` | `TableSharedSharded` | Locking strategy |
| `build_tableshared_type` | `BuildTableSharedSharded` | Factory for the shared table |
