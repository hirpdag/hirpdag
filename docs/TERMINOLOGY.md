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

## Table interfaces

### `ThreadUnsafeTable<D, R, WR>`
The single-threaded storage unit for a hash-consing table.  Implementations differ in lookup strategy and memory layout.  `WR` names the weak-reference type the table evicts against; every inner table stores weak references and can purge dead entries.

Trait: `hirpdag_hashconsing::ThreadUnsafeTable`

### `Table<D, R, WR>`
The thread-safe hash-consing interface (`get` / `get_or_insert` over `&self`).  Implementations choose their concurrency strategy: some wrap one or more inner single-threaded `ThreadUnsafeTable` instances behind locks, others store the mapping directly in a concurrent collection.  Note the trait is *not* parameterized over an inner `ThreadUnsafeTable` — a backend that needs one (mutex/sharded) carries it as its own generic, so backends that don't (the concurrent-collection ones) name no table at all.  `WR` appears in the trait, not the backend types, so a backend implements `Table` generically over the weak type.  A concurrent backend that implements `Table` directly stores **strong** references (retain-forever); such presets are named `*_strong`.

Trait: `hirpdag_hashconsing::Table`

### `NonPurgingTable<D, R, WR>`
A concurrent hash-consing map that stores **weak** references (`WR: ReferenceWeak<D, R>`) but performs no purging of dead entries on its own.  Lookups upgrade the stored weak reference; the name states the invariant.  Wrapping one in `TableAmortizedPurge` adds purging and yields a `Table`.  The concurrent third-party backends implement this directly, alongside a direct strong-retention `Table`; both views share the backend's map plumbing through private inherent methods, and the weak side stores a `WeakEntryStrong` holder that makes a weak reference `Clone + Hash + Eq` by stable pointer identity.

Trait: `hirpdag_hashconsing::NonPurgingTable`

### How the interfaces relate

`Table` is the interface every concrete table ultimately presents (it is what the `hirpdag` macro selects and the hash-consing machinery calls).  The other interfaces reach it either directly or through an adapter:

```text
  ThreadUnsafeTable<D,R,WR>     ── TableSharedMutex / TableSharedSharded ──▶  Table<D,R,WR>
   single-threaded; weak refs;     (lock adapters)
   purges

  NonPurgingTable<D,R,WR>       ── TableAmortizedPurge ───────────────────▶  Table<D,R,WR>
   concurrent; weak refs;          (adds amortized purge)
   no purge

  concurrent backend (strong)  ── implements Table directly ──────────────▶  Table<D,R,WR>
   strong refs; never evicts       (retain-forever; `*_strong` presets)
```

The concurrent backends (dashmap, flurry, …) offer two views of the *same* collection: a direct strong-retention `Table` (values held strongly, retained forever) and a `NonPurgingTable` (values held weakly, purged on demand by the adapter).  They share their map plumbing through private inherent methods.

## Table implementations

### `TableAmortizedPurge<D, R, WR, S>`
Adapter turning a `NonPurgingTable` `S` into a purging `Table`.  It adds **no synchronization of its own** — `get` and `get_or_insert` delegate straight to the backend, whose `get_or_insert` is atomic via its native concurrency (dashmap's per-shard entry lock, skipmap's `compare_insert`, flurry's `try_insert`/`compute_if_present`, arc-swap's inherent writer serialization), so concurrent interning never yields two live nodes for one key.  Dead entries are purged **amortized** — a single `retain_alive` sweep once the map grows past twice its size at the previous purge; the threshold is a lock-free `AtomicUsize` and a `compare_exchange` elects one sweeper while other writers continue.  This is how the otherwise strong-only concurrent backends gain weak-key hash-consing.

Source: `hirpdag_hashconsing/src/table/amortized_purge.rs`

### `WeakEntry`
The per-element storage unit inside vector-backed tables: a precomputed hash plus a weak reference.  The cached hash allows O(1) filtering before the equality check.

Source: `hirpdag_hashconsing/src/table/weak_entry.rs`

### `TableVecLinearWeak`
Unsorted `Vec` of weak entries; O(n) linear search.  Simple, allocation-friendly, suitable for small node sets.

### `TableVecSortedWeak`
Hash-sorted `Vec` of weak entries; O(log n) binary search to the hash run, then O(k) linear scan within the run.  Better than linear for medium node sets.

### `TableHashmapFallbackWeak`
`HashMap`-based table with a fallback to an alternate table for small sizes.  O(1) average lookup; the fallback handles the cold start efficiently.

### `TableSharedMutex`
`Table` implementation wrapping a single `Mutex`.  Simple; all threads serialise on one lock.  An adapter that connects a single-threaded `ThreadUnsafeTable` to the thread-safe `Table` interface.

### `TableSharedSharded`
`Table` implementation using `N_SHARDS` (= 8) independent mutexes.  Threads hashing to different shards never contend.  Shard selection is `hash & (N_SHARDS - 1)` — a bitmask because `N_SHARDS` is a power of two.  Like the mutex backend, an adapter connecting a `ThreadUnsafeTable` to the `Table` interface.

Source: `hirpdag_hashconsing/src/table/shared_sharded.rs`

### Third-party-collection table backends (`third-party-tables` feature)
Table backends built on external collection crates, behind the opt-in `third-party-tables` Cargo feature (off by default; enable it on `hirpdag` to select these presets). `TableTovWeakTable` is an inner `ThreadUnsafeTable` (wrapping the [`weak-table`] crate) used behind the sharded shared table; the rest store the interned mapping directly in a concurrent collection instead of delegating to an inner `ThreadUnsafeTable`. Each of these concurrent backends implements `Table` directly (a strong-reference view) and `NonPurgingTable` (a weak-reference view), and so has **two presets**: the `*_strong` preset uses the direct strong `Table`, which retains unreferenced nodes rather than garbage-collecting them; the un-suffixed preset wraps the backend's `NonPurgingTable` in `TableAmortizedPurge`, yielding a weak-key, purging `Table` that evicts dead nodes. They require a `Send + Sync` reference, so they are wired to `RefArc`.

| Type | Purging preset | Strong preset | Backend | Strategy |
| --- | --- | --- | --- | --- |
| `TableTovWeakTable` | `arc_tovweaktable` | — | [`weak-table`] | `WeakHashSet` inner `ThreadUnsafeTable` behind `TableSharedSharded`; weak-reference GC. |
| `TableSharedDashMap` | `arc_dashmap` | `arc_dashmap_strong` | [`dashmap`] | Bucket-striped concurrent hash map; per-shard locks. |
| `TableSharedFlurry` | `arc_flurry` | `arc_flurry_strong` | [`flurry`] | Lock-free hash map (Java `ConcurrentHashMap` port); keys must be `Ord`. |
| `TableSharedSkipMap` | `arc_skipmap` | `arc_skipmap_strong` | [`crossbeam-skiplist`] | Lock-free ordered skip list; `O(log n)` lookup, no hasher. |
| `TableSharedArcSwap` | `arc_arcswap` | `arc_arcswap_strong` | [`arc-swap`] | RCU / copy-on-write: lock-free reads, whole-map clone per insert (`O(n)` writes). |

[`weak-table`]: https://crates.io/crates/weak-table
[`dashmap`]: https://crates.io/crates/dashmap
[`flurry`]: https://crates.io/crates/flurry
[`crossbeam-skiplist`]: https://crates.io/crates/crossbeam-skiplist
[`arc-swap`]: https://crates.io/crates/arc-swap

Source: `hirpdag_hashconsing/src/table/tov_weak_table_threadunsafe.rs`, `dashmap_strong.rs`, `flurry_strong.rs`, `skipmap_strong.rs`, `arcswap_strong.rs`

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
